//! Conversation controls and fingerprint-bound identity verification.
use crate::{
    Pending, State,
    model::{self, ConversationChange, IdentityInfo, IdentityPanel},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
impl State {
    pub(crate) fn start_new_group(&mut self) {
        self.new_chat = false;
        self.new_group = true;
        self.group_name_input.clear();
        self.group_member_input.clear();
        self.group_participants.clear();
    }
    pub(crate) fn add_group_participant(&mut self) {
        let raw = self.group_member_input.text.trim();
        let Some(id) = model::recipient_id(raw).or_else(|| {
            self.conversations
                .iter()
                .find(|c| !c.group && c.name.eq_ignore_ascii_case(raw))
                .map(|c| c.id.clone())
        }) else {
            self.error = Some("Enter a contact name, international number or account UUID.".into());
            return;
        };
        let id = self.resolve_id(&id);
        if !self.group_participants.contains(&id)
            && self.account.as_ref() != Some(&id)
            && self.group_participants.len() < 1000
        {
            self.group_participants.push(id);
        }
        self.group_member_input.clear();
    }
    pub(crate) fn toggle_group_participant(&mut self, id: &str) {
        if self.account.as_deref() == Some(id) {
            return;
        }
        if let Some(index) = self.group_participants.iter().position(|p| p == id) {
            self.group_participants.remove(index);
        } else if self.group_participants.len() < 1000 {
            self.group_participants.push(id.into());
        }
    }
    pub(crate) fn create_group(&mut self) {
        let name = self.group_name_input.text.trim();
        if name.is_empty()
            || self.group_participants.is_empty()
            || name.len() > 1024
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::CreatedGroup))
        {
            return;
        }
        self.request(
            "updateGroup",
            json!({"name":name,"member":self.group_participants}),
            Pending::CreatedGroup,
        );
    }
    pub(crate) fn group_joined(&mut self, result: &Value) {
        let Some(id) = result["groupId"].as_str() else {
            self.error = Some(
                "The engine did not confirm the group. Refresh groups before retrying.".into(),
            );
            return;
        };
        let failed = result["results"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|r| r["type"].as_str().is_some_and(|kind| kind != "SUCCESS"))
        });
        if failed {
            self.error=Some("The group request did not complete for every recipient. Refresh the group before retrying.".into());
        }
        if !self.conversations.iter().any(|c| c.id == id) {
            let name = if self.group_name_input.text.trim().is_empty() {
                "Group".into()
            } else {
                self.group_name_input.text.trim().into()
            };
            self.conversations
                .push(crate::Conversation::new(id.into(), name, true));
        }
        self.new_group = false;
        self.new_chat = false;
        self.choose(id.into());
        self.persist();
        self.request("listGroups", json!({}), Pending::Groups);
    }
    pub(crate) fn open_conversation_info(&mut self, peer: &str) {
        let Some(c) = self.conversations.iter().find(|c| c.id == peer) else {
            return;
        };
        self.conversation_name.replace_text(c.name.clone());
        self.group_description_input
            .replace_text(c.group_description.clone());
        self.group_member_input.clear();
        self.conversation_info = Some(peer.into());
        self.stop_typing();
    }
    pub(crate) fn group_operation(&mut self, peer: &str, op: model::GroupOperation) {
        let Some(c) = self
            .conversations
            .iter()
            .find(|c| c.id == peer && c.group && c.group_member && !c.group_terminated)
        else {
            return;
        };
        if self
            .pending
            .values()
            .any(|p| matches!(p,Pending::GroupUpdate(id) if id==peer))
        {
            return;
        }
        let admin = self
            .account
            .as_ref()
            .is_some_and(|a| c.group_admins.contains(a));
        let permitted = match &op {
            model::GroupOperation::Leave => true,
            model::GroupOperation::Add(_) => admin || c.permission_add == "EVERY_MEMBER",
            model::GroupOperation::Description(_) => admin || c.permission_edit == "EVERY_MEMBER",
            _ => admin,
        };
        if !permitted {
            self.error = Some("Only group admins can make this change.".into());
            return;
        }
        let mut params = json!({"groupId":peer});
        let mut method = "updateGroup";
        let member_key = match &op {
            model::GroupOperation::Add(_) => Some("member"),
            model::GroupOperation::Remove(_) => Some("removeMember"),
            model::GroupOperation::Promote(_) => Some("admin"),
            model::GroupOperation::Demote(_) => Some("removeAdmin"),
            model::GroupOperation::Ban(_) => Some("ban"),
            model::GroupOperation::Unban(_) => Some("unban"),
            _ => None,
        };
        match op {
            model::GroupOperation::Add(raw)
            | model::GroupOperation::Remove(raw)
            | model::GroupOperation::Promote(raw)
            | model::GroupOperation::Demote(raw)
            | model::GroupOperation::Ban(raw)
            | model::GroupOperation::Unban(raw) => {
                let key = member_key.expect("member operation");
                let Some(id) = model::recipient_id(raw.trim())
                    .or_else(|| {
                        self.conversations
                            .iter()
                            .find(|c| !c.group && c.name.eq_ignore_ascii_case(raw.trim()))
                            .map(|c| c.id.clone())
                    })
                    .or_else(|| {
                        c.members
                            .iter()
                            .chain(&c.group_pending)
                            .chain(&c.group_requesting)
                            .chain(&c.group_banned)
                            .find(|id| *id == &raw)
                            .cloned()
                    })
                else {
                    self.error = Some(
                        "Choose a contact or enter an international number or account UUID.".into(),
                    );
                    return;
                };
                params[key] = json!([id]);
            }
            model::GroupOperation::Description(description) => {
                if description.len() > 8192 {
                    self.error = Some("The group description is too long.".into());
                    return;
                }
                params["description"] = json!(description);
            }
            model::GroupOperation::Link(state) => params["link"] = json!(state),
            model::GroupOperation::ResetLink => params["resetLink"] = json!(true),
            model::GroupOperation::Permission(key, value) => params[key] = json!(value),
            model::GroupOperation::Leave => method = "quitGroup",
        }
        self.group_confirmation = None;
        self.request(method, params, Pending::GroupUpdate(peer.into()));
    }
    pub(crate) fn group_updated(&mut self, _peer: &str, result: &Value) {
        if result["results"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|r| r["type"].as_str().is_some_and(|kind| kind != "SUCCESS"))
        }) {
            self.error = Some(
                "The group change could not be delivered. Refresh the group and retry.".into(),
            );
        }
        self.group_member_input.clear();
        self.group_member_panel = None;
        self.request("listGroups", json!({}), Pending::Groups);
    }
    pub(crate) fn change_conversation(&mut self, peer: &str, change: ConversationChange) {
        let Some(c) = self.conversations.iter().find(|c| c.id == peer) else {
            return;
        };
        if self
            .pending
            .values()
            .any(|p| matches!(p,Pending::ConversationUpdate(id,_) if id==peer))
        {
            return;
        }
        let group = c.group;
        let (method, params) = match &change {
            ConversationChange::Block(block) => {
                let params = if group {
                    json!({"groupId":[peer]})
                } else {
                    json!({"recipient":[peer]})
                };
                (if *block { "block" } else { "unblock" }, params)
            }
            ConversationChange::Expiration(seconds) => {
                if *seconds > i32::MAX as u32 {
                    return;
                }
                (
                    if group {
                        "updateGroup"
                    } else {
                        "updateContact"
                    },
                    if group {
                        json!({"groupId":peer,"expiration":seconds})
                    } else {
                        json!({"recipient":peer,"expiration":seconds})
                    },
                )
            }
            ConversationChange::Rename(name) => {
                if name.trim().is_empty() || name.len() > 1024 {
                    return;
                }
                (
                    if group {
                        "updateGroup"
                    } else {
                        "updateContact"
                    },
                    if group {
                        json!({"groupId":peer,"name":name})
                    } else {
                        json!({"recipient":peer,"name":name})
                    },
                )
            }
        };
        self.request(
            method,
            params,
            Pending::ConversationUpdate(peer.into(), change),
        );
    }
    pub(crate) fn conversation_updated(&mut self, peer: &str, change: ConversationChange) {
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == peer) {
            match change {
                ConversationChange::Block(block) => {
                    c.blocked = block;
                    if block {
                        c.typing.clear();
                    }
                }
                ConversationChange::Expiration(seconds) => {
                    c.expiration = seconds;
                    c.expiration_updated_at = model::now_ms();
                }
                ConversationChange::Rename(name) => c.name = name,
            }
        }
        self.dirty = true;
        self.persist();
        self.request("listContacts", json!({}), Pending::Contacts);
        self.request("listGroups", json!({}), Pending::Groups);
    }
    pub(crate) fn mute_conversation(&mut self, peer: &str, seconds: Option<u64>) {
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == peer) {
            c.muted_until = seconds.map(|s| model::now_ms().saturating_add(s.saturating_mul(1000)));
            self.dirty = true;
            self.persist();
        }
    }
    pub(crate) fn open_identity(&mut self, peer: &str) {
        if self.conversations.iter().any(|c| c.id == peer && c.group)
            || self.account.as_deref() == Some(peer)
        {
            return;
        }
        self.identity_panel = Some(IdentityPanel {
            peer: peer.into(),
            identity: None,
            loading: true,
            saving: false,
            error: None,
        });
        self.request_identity(peer);
    }
    pub(crate) fn request_identity(&mut self, peer: &str) {
        if let Some(panel) = self.identity_panel.as_mut().filter(|p| p.peer == peer) {
            panel.loading = true;
            panel.saving = false;
        }
        if self
            .request(
                "listIdentities",
                json!({"number":peer}),
                Pending::Identities(peer.into()),
            )
            .is_none()
            && let Some(panel) = &mut self.identity_panel
        {
            panel.loading = false;
            panel.error = Some("Connect to Signal to load the current safety number.".into());
        }
    }
    pub(crate) fn identity_result(&mut self, peer: &str, value: &Value) {
        let Some(panel) = self.identity_panel.as_mut().filter(|p| p.peer == peer) else {
            return;
        };
        panel.loading = false;
        panel.saving = false;
        let aliases = self
            .conversations
            .iter()
            .find(|c| c.id == peer)
            .map(|c| c.aliases.clone())
            .unwrap_or_default();
        let identity = value
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| {
                [i["uuid"].as_str(), i["number"].as_str()]
                    .into_iter()
                    .flatten()
                    .any(|id| id == peer || aliases.iter().any(|a| a == id))
            })
            .max_by_key(|i| i["addedTimestamp"].as_u64().unwrap_or(0));
        panel.identity = identity.and_then(parse_identity);
        panel.error=panel.identity.is_none().then(||"A safety number is not available yet. Exchange a message with this contact, then refresh.".into());
    }
    pub(crate) fn verify_identity(&mut self) {
        let Some(panel) = &mut self.identity_panel else {
            return;
        };
        let Some(identity) = &panel.identity else {
            return;
        };
        if panel.loading || panel.saving {
            return;
        }
        let peer = panel.peer.clone();
        let verified = !identity.verified;
        let safety = identity.safety_number.clone();
        panel.saving = true;
        panel.error = None;
        let method = if verified {
            "trust"
        } else {
            "setIdentityUnverified"
        };
        let params = json!({"recipient":peer,"verifiedSafetyNumber":safety});
        if self
            .request(method, params, Pending::VerifyIdentity(peer))
            .is_none()
            && let Some(panel) = &mut self.identity_panel
        {
            panel.saving = false;
            panel.error = Some("Connect to Signal before changing verification.".into());
        }
    }
    pub(crate) fn identity_error(&mut self, pending: &Pending) -> bool {
        let peer = match pending {
            Pending::Identities(peer) | Pending::VerifyIdentity(peer) => peer,
            _ => return false,
        };
        if let Some(panel) = self.identity_panel.as_mut().filter(|p| &p.peer == peer) {
            panel.loading = false;
            panel.saving = false;
            panel.error=Some("Safety number could not be checked or changed. Refresh before retrying; a changed identity must be compared again.".into());
        }
        true
    }
}
fn parse_identity(v: &Value) -> Option<IdentityInfo> {
    let (verified, trusted) = match v["trustLevel"].as_str()? {
        "TRUSTED_VERIFIED" => (true, true),
        "TRUSTED_UNVERIFIED" => (false, true),
        "UNTRUSTED" => (false, false),
        _ => return None,
    };
    let safety: String = v["safetyNumber"]
        .as_str()?
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if safety.len() != 60 || !safety.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fingerprint = v["fingerprint"].as_str()?.to_owned();
    if fingerprint.is_empty()
        || fingerprint.len() > 256
        || !fingerprint
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c.is_whitespace() || c == ':')
    {
        return None;
    }
    Some(IdentityInfo {
        safety_number: safety,
        scannable: v["scannableSafetyNumber"]
            .as_str()
            .filter(|s| s.len() < 4096)
            .and_then(|s| STANDARD.decode(s).ok()),
        verified,
        trusted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_safety_number_is_never_presented_as_verifiable() {
        for text in ["12345", "a".repeat(60).as_str()] {
            assert!(parse_identity(&json!({"safetyNumber":text,"fingerprint":"abcd"})).is_none());
        }
        assert!(parse_identity(&json!({"safetyNumber":"12345 ".repeat(12),"fingerprint":"abcd","trustLevel":"TRUSTED_VERIFIED"})).unwrap().verified);
    }
    #[test]
    #[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
    fn opening_identity_is_read_only_and_verification_binds_the_displayed_number() {
        let (bridge, requests, _) = crate::transport::Bridge::mock();
        let mut s = State::default();
        s.account = Some("self".into());
        s.bridge = Some(bridge);
        s.conversations.push(crate::Conversation::new(
            "peer".into(),
            "Peer".into(),
            false,
        ));
        s.open_identity("peer");
        let read = requests.try_recv().unwrap();
        assert_eq!(read["method"], "listIdentities");
        assert!(requests.try_recv().is_err());
        let safety = "1234567890".repeat(6);
        s.identity_result("peer",&json!([{"uuid":"peer","safetyNumber":safety,"fingerprint":"abcd","trustLevel":"TRUSTED_UNVERIFIED"}]));
        s.verify_identity();
        let verify = requests.try_recv().unwrap();
        assert_eq!(verify["method"], "trust");
        assert_eq!(verify["params"]["verifiedSafetyNumber"], safety);
        assert!(verify["params"].get("trustAllKnownKeys").is_none());
        assert!(
            !s.identity_panel
                .as_ref()
                .unwrap()
                .identity
                .as_ref()
                .unwrap()
                .verified,
            "Verification changes only after the engine confirms it"
        );
        s.identity_result("peer",&json!([{"uuid":"peer","safetyNumber":safety,"fingerprint":"abcd","trustLevel":"TRUSTED_VERIFIED"}]));
        s.verify_identity();
        let unverify = requests.try_recv().unwrap();
        assert_eq!(unverify["method"], "setIdentityUnverified");
        assert_eq!(unverify["params"]["verifiedSafetyNumber"], safety);
    }
    #[test]
    #[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
    fn conversation_changes_wait_for_success_and_use_group_protocol_parameters() {
        let (bridge, requests, _) = crate::transport::Bridge::mock();
        let mut s = State::default();
        s.account = Some("self".into());
        s.bridge = Some(bridge);
        s.conversations.push(crate::Conversation::new(
            "group-id".into(),
            "Group".into(),
            true,
        ));
        s.change_conversation("group-id", ConversationChange::Expiration(86400));
        let request = requests.try_recv().unwrap();
        assert_eq!(request["method"], "updateGroup");
        assert_eq!(request["params"]["groupId"], "group-id");
        assert_eq!(s.conversations[0].expiration, 0);
        s.event(
            json!({"id":request["id"],"error":{"code":-1}}),
            &mut rune::Tx::default(),
        );
        assert_eq!(s.conversations[0].expiration, 0);
        s.change_conversation("group-id", ConversationChange::Block(true));
        let request = requests.try_recv().unwrap();
        assert_eq!(request["params"]["groupId"], json!(["group-id"]));
        assert!(!s.conversations[0].blocked);
    }
    #[test]
    #[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
    fn group_roles_and_requests_use_authorized_protocol_operations() {
        let (bridge, requests, _) = crate::transport::Bridge::mock();
        let mut s = State::default();
        s.account = Some("self".into());
        s.bridge = Some(bridge);
        let mut group = crate::Conversation::new("group".into(), "Group".into(), true);
        group.group_member = true;
        group.members = vec!["self".into(), "peer".into()];
        group.group_requesting = vec!["requester".into()];
        s.conversations.push(group);
        s.group_operation("group", model::GroupOperation::Promote("peer".into()));
        assert!(requests.try_recv().is_err());
        s.conversations[0].group_admins.push("self".into());
        s.group_operation("group", model::GroupOperation::Promote("peer".into()));
        let request = requests.try_recv().unwrap();
        assert_eq!(request["method"], "updateGroup");
        assert_eq!(request["params"]["admin"], json!(["peer"]));
        s.pending.clear();
        s.group_operation("group", model::GroupOperation::Add("requester".into()));
        let request = requests.try_recv().unwrap();
        assert_eq!(request["params"]["member"], json!(["requester"]));
        assert_eq!(
            s.conversations[0].group_requesting,
            vec!["requester"],
            "Server success and refreshed group state are required before changing the UI"
        );
        s.pending.clear();
        s.group_operation("group", model::GroupOperation::Leave);
        let request = requests.try_recv().unwrap();
        assert_eq!(request["method"], "quitGroup");
        assert_eq!(request["params"]["groupId"], "group");
        assert!(s.conversations[0].group_member);
    }
    #[test]
    #[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
    fn group_creation_waits_for_confirmation_and_uses_selected_contacts() {
        let (bridge, requests, _) = crate::transport::Bridge::mock();
        let mut s = State::default();
        s.account = Some("self".into());
        s.bridge = Some(bridge);
        s.start_new_group();
        s.group_name_input.replace_text("Weekend");
        s.group_participants = vec!["+12025550123".into()];
        s.create_group();
        let request = requests.try_recv().unwrap();
        assert_eq!(request["method"], "updateGroup");
        assert!(request["params"].get("groupId").is_none());
        assert_eq!(request["params"]["name"], "Weekend");
        assert_eq!(request["params"]["member"], json!(["+12025550123"]));
        assert!(s.conversations.is_empty());
        s.event(
            json!({"id":request["id"],"result":{"groupId":"confirmed-id","results":[]}}),
            &mut rune::Tx::default(),
        );
        assert_eq!(s.selected.as_deref(), Some("confirmed-id"));
        assert!(!s.new_group);
        assert!(s.conversations[0].group);
    }
}
