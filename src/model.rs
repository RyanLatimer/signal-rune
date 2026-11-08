use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delivery {
    Pending,
    #[default]
    Sent,
    Delivered,
    Read,
    Failed,
}
impl Delivery {
    pub fn advance(&mut self, next: Self) {
        let rank = |d: Self| match d {
            Self::Pending => 0,
            Self::Sent | Self::Failed => 1,
            Self::Delivered => 2,
            Self::Read => 3,
        };
        if rank(next) > rank(*self) {
            *self = next;
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub media_id: Option<String>,
    pub voice_note: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Quote {
    pub timestamp: u64,
    pub author: String,
    pub body: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: u64,
    pub body: String,
    pub outgoing: bool,
    pub timestamp: u64,
    pub delivery: Delivery,
    pub sender: String,
    pub read: bool,
    pub read_receipt_sent: bool,
    pub receipts: BTreeMap<String, Delivery>,
    pub expires_in: u32,
    pub expires_at: Option<u64>,
    pub edited: bool,
    pub deleted: bool,
    pub hidden: bool,
    pub tombstone_timestamps: Vec<u64>,
    pub quote: Option<Quote>,
    pub attachments: Vec<Attachment>,
    pub reactions: BTreeMap<String, String>,
    pub edited_at: Option<u64>,
    pub edit_history: Vec<EditVersion>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditVersion {
    pub timestamp: u64,
    pub body: String,
    pub quote: Option<Quote>,
    pub delivery: Delivery,
    pub receipts: BTreeMap<String, Delivery>,
}
impl Message {
    fn merge_duplicate(&mut self, other: Self) {
        let old = self.clone();
        let same = old.latest_timestamp() == other.latest_timestamp();
        if other.latest_timestamp() > old.latest_timestamp() {
            *self = other.clone();
        }
        self.id = old.id.min(other.id);
        if same {
            self.read = old.read || other.read;
            self.read_receipt_sent = old.read_receipt_sent || other.read_receipt_sent;
            self.delivery.advance(other.delivery);
            for (sender, status) in &other.receipts {
                self.receipts
                    .entry(sender.clone())
                    .or_default()
                    .advance(*status);
            }
        }
        self.hidden = old.hidden || other.hidden;
        self.deleted = old.deleted || other.deleted;
        self.expires_at = old.expires_at.into_iter().chain(other.expires_at).min();
        self.expires_in = [old.expires_in, other.expires_in]
            .into_iter()
            .filter(|n| *n > 0)
            .min()
            .unwrap_or(0);
        self.tombstone_timestamps.extend(
            old.tombstone_timestamps
                .iter()
                .chain(&other.tombstone_timestamps),
        );
        for version in [&old, &other] {
            self.tombstone_timestamps.push(version.latest_timestamp());
            for a in &version.attachments {
                if let Some(current) = self
                    .attachments
                    .iter_mut()
                    .find(|current| current.id == a.id && current.name == a.name)
                    && current.media_id.is_none()
                {
                    current.media_id = a.media_id.clone();
                }
            }
            for historical in &version.edit_history {
                if let Some(current) = self
                    .edit_history
                    .iter_mut()
                    .find(|v| v.timestamp == historical.timestamp)
                {
                    current.delivery.advance(historical.delivery);
                    for (sender, status) in &historical.receipts {
                        current
                            .receipts
                            .entry(sender.clone())
                            .or_default()
                            .advance(*status);
                    }
                } else if historical.timestamp < self.latest_timestamp() {
                    self.edit_history.push(historical.clone());
                }
            }
            if version.latest_timestamp() < self.latest_timestamp()
                && !self
                    .edit_history
                    .iter()
                    .any(|v| v.timestamp == version.latest_timestamp())
            {
                self.edit_history.push(EditVersion {
                    timestamp: version.latest_timestamp(),
                    body: version.body.clone(),
                    quote: version.quote.clone(),
                    delivery: version.delivery,
                    receipts: version.receipts.clone(),
                });
            }
        }
        self.tombstone_timestamps.sort_unstable();
        self.tombstone_timestamps.dedup();
        self.edit_history.sort_by_key(|v| v.timestamp);
        if self.hidden || self.deleted {
            self.body.clear();
            self.quote = None;
            self.attachments.clear();
            self.reactions.clear();
            self.edit_history.clear();
            self.read = true;
            self.read_receipt_sent = true;
        }
    }
    pub fn start_expiration(&mut self, at: u64) {
        if self.expires_in == 0 || self.hidden {
            return;
        }
        let deadline = at.saturating_add(u64::from(self.expires_in) * 1000);
        self.expires_at = Some(self.expires_at.map_or(deadline, |old| old.min(deadline)));
    }
    pub fn latest_timestamp(&self) -> u64 {
        self.edited_at.unwrap_or(self.timestamp)
    }
    pub fn matches_timestamp(&self, timestamp: u64) -> bool {
        self.tombstone_timestamps.contains(&timestamp)
            || self.timestamp == timestamp
            || self.edited_at == Some(timestamp)
            || self.edit_history.iter().any(|v| v.timestamp == timestamp)
    }
    pub fn can_edit(&self, note_to_self: bool) -> bool {
        self.outgoing
            && !self.deleted
            && !self.hidden
            && !self.body.is_empty()
            && self.attachments.is_empty()
            && !matches!(self.delivery, Delivery::Pending | Delivery::Failed)
            && self.edit_history.len() < 10
            && (note_to_self || now_ms().saturating_sub(self.timestamp) < 86_400_000)
    }
    pub fn can_delete_for_everyone(&self) -> bool {
        self.outgoing
            && !self.deleted
            && !self.hidden
            && self.delivery != Delivery::Pending
            && now_ms().saturating_sub(self.timestamp) < 86_400_000
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ActionKind {
    Reaction { emoji: String, remove: bool },
    Edit { body: String, quote: Option<Quote> },
    Delete,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueuedAction {
    pub id: u64,
    pub conversation: String,
    pub message: u64,
    pub target: u64,
    pub author: String,
    pub kind: ActionKind,
    pub failed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mutation {
    pub conversation: String,
    pub author: String,
    pub actor: String,
    pub target: u64,
    pub at: u64,
    pub kind: ActionKind,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DraftBackup {
    pub body: String,
    pub quote: Option<Quote>,
}
#[derive(Clone, Debug)]
pub struct TypingPresence {
    pub name: String,
    pub until: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub group: bool,
    pub active: bool,
    pub aliases: Vec<String>,
    pub messages: Vec<Message>,
    pub unread: u32,
    pub draft: String,
    pub draft_quote: Option<Quote>,
    pub draft_edit: Option<u64>,
    pub draft_backup: Option<DraftBackup>,
    pub draft_attachments: Vec<Attachment>,
    pub pinned: u64,
    pub archived: bool,
    pub blocked: bool,
    pub muted_until: Option<u64>,
    pub expiration: u32,
    pub expiration_updated_at: u64,
    pub members: Vec<String>,
    pub group_description: String,
    pub group_admins: Vec<String>,
    pub group_pending: Vec<String>,
    pub group_requesting: Vec<String>,
    pub group_banned: Vec<String>,
    pub group_link: Option<String>,
    pub group_member: bool,
    pub group_terminated: bool,
    pub permission_edit: String,
    pub permission_add: String,
    pub permission_send: String,
    #[serde(skip)]
    pub typing: HashMap<String, TypingPresence>,
}
#[derive(Clone)]
pub enum ConversationChange {
    Block(bool),
    Expiration(u32),
    Rename(String),
}
#[derive(Clone)]
pub enum GroupOperation {
    Add(String),
    Remove(String),
    Promote(String),
    Demote(String),
    Ban(String),
    Unban(String),
    Description(String),
    Link(&'static str),
    ResetLink,
    Permission(&'static str, &'static str),
    Leave,
}
#[derive(Clone)]
pub struct IdentityInfo {
    pub safety_number: String,
    pub scannable: Option<Vec<u8>>,
    pub verified: bool,
    pub trusted: bool,
}
pub struct IdentityPanel {
    pub peer: String,
    pub identity: Option<IdentityInfo>,
    pub loading: bool,
    pub saving: bool,
    pub error: Option<String>,
}
impl Conversation {
    pub fn new(id: String, name: String, group: bool) -> Self {
        Self {
            aliases: vec![id.clone()],
            id,
            name,
            group,
            ..Self::default()
        }
    }
    pub fn last_message(&self) -> Option<&Message> {
        self.messages.iter().rev().find(|m| !m.hidden)
    }
    pub fn matches(&self, id: &str) -> bool {
        self.id == id || self.aliases.iter().any(|a| a == id)
    }
    pub fn recount_unread(&mut self) {
        self.unread = self
            .messages
            .iter()
            .filter(|m| !m.outgoing && !m.read && !m.hidden)
            .count() as u32;
    }
    pub fn sort_messages(&mut self) {
        self.messages.sort_by_key(|m| (m.timestamp, m.id));
    }
    pub fn reconcile_duplicates(&mut self) -> HashMap<u64, u64> {
        self.sort_messages();
        let mut unique: Vec<Message> = Vec::new();
        let mut indexes = HashMap::new();
        let mut remap = HashMap::new();
        for message in std::mem::take(&mut self.messages) {
            let identity = (message.timestamp, message.outgoing, message.sender.clone());
            if let Some(index) = indexes.get(&identity).copied() {
                let previous: &mut Message = &mut unique[index];
                let old = message.id;
                previous.merge_duplicate(message);
                remap.insert(old, previous.id);
            } else {
                indexes.insert(identity, unique.len());
                unique.push(message);
            }
        }
        self.messages = unique;
        self.recount_unread();
        remap
    }
    pub fn clear_deleted_edit_draft(&mut self) {
        if self.draft_edit.is_some_and(|id| {
            self.messages
                .iter()
                .any(|m| m.id == id && (m.hidden || m.deleted))
        }) {
            let backup = self.draft_backup.take().unwrap_or_default();
            self.draft = backup.body;
            self.draft_quote = backup.quote;
            self.draft_edit = None;
        }
    }
    pub fn apply_receipt(&mut self, sender: &str, timestamps: &[u64], delivery: Delivery) -> bool {
        let mut changed = false;
        for m in &mut self.messages {
            if !m.outgoing {
                continue;
            }
            for version in &mut m.edit_history {
                if timestamps.contains(&version.timestamp) {
                    version.delivery.advance(delivery);
                    version
                        .receipts
                        .entry(sender.to_owned())
                        .or_default()
                        .advance(delivery);
                }
            }
            if timestamps.contains(&m.latest_timestamp()) {
                let before = m.delivery;
                m.delivery.advance(delivery);
                m.receipts
                    .entry(sender.to_owned())
                    .or_default()
                    .advance(delivery);
                changed |= before != m.delivery;
            }
        }
        changed
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub dark: bool,
    pub typing_indicators: bool,
    pub read_receipts: bool,
    pub notifications: bool,
    pub last_outgoing_timestamp: u64,
    pub next_message_id: u64,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            dark: true,
            typing_indicators: true,
            read_receipts: true,
            notifications: true,
            last_outgoing_timestamp: 0,
            next_message_id: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfiguration {
    pub version: u32,
    pub read_receipts: bool,
    pub typing_indicators: bool,
    pub link_previews: bool,
    pub unidentified_delivery_indicators: bool,
}
impl Default for EngineConfiguration {
    fn default() -> Self {
        Self {
            version: 1,
            read_receipts: true,
            typing_indicators: true,
            link_previews: true,
            unidentified_delivery_indicators: false,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncLedger {
    pub receipts: BTreeMap<String, BTreeMap<u64, Delivery>>,
    pub read_through: BTreeMap<String, u64>,
    pub read_at: BTreeMap<String, BTreeMap<u64, u64>>,
    pub actions: Vec<QueuedAction>,
    pub mutations: Vec<Mutation>,
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn envelope(v: &Value) -> Option<&Value> {
    let p = &v["params"];
    p.get("envelope")
        .or_else(|| p.get("result").and_then(|r| r.get("envelope")))
}
pub fn source(e: &Value) -> Option<&str> {
    e["sourceUuid"]
        .as_str()
        .or(e["sourceNumber"].as_str())
        .or(e["source"].as_str())
}
pub fn account(v: &Value) -> Option<&str> {
    v["params"]["account"]
        .as_str()
        .or(v["params"]["result"]["account"].as_str())
}
pub fn account_id(v: &Value) -> Option<String> {
    v["aci"]
        .as_str()
        .or(v["number"].as_str())
        .map(str::to_owned)
}
pub struct DataContext<'a> {
    pub conversation: String,
    pub name: String,
    pub group: bool,
    pub aliases: Vec<String>,
    pub sender: String,
    pub outgoing: bool,
    pub data: &'a Value,
    pub edit_target: Option<u64>,
}
pub fn data_context(v: &Value) -> Option<DataContext<'_>> {
    let e = envelope(v)?;
    let sent = e["syncMessage"].get("sentMessage");
    let outgoing = sent.is_some();
    let edit = if outgoing {
        sent?.get("editMessage")
    } else {
        e.get("editMessage")
    };
    let data = edit
        .and_then(|edit| edit.get("dataMessage"))
        .or_else(|| if outgoing { sent } else { e.get("dataMessage") })?;
    let group_id = data["groupInfo"]["groupId"]
        .as_str()
        .or(data["groupV2"]["id"].as_str());
    let sender = source(e).unwrap_or_default().to_owned();
    let destination = sent.and_then(|m| {
        m["destinationUuid"]
            .as_str()
            .or(m["destinationNumber"].as_str())
            .or(m["destination"].as_str())
    });
    let peer = if outgoing {
        destination.or_else(|| source(e))
    } else {
        Some(sender.as_str())
    };
    let conversation = group_id.or(peer)?.to_owned();
    if conversation.is_empty() {
        return None;
    }
    let name = if group_id.is_some() {
        data["groupInfo"]["groupName"]
            .as_str()
            .or(data["groupInfo"]["name"].as_str())
            .unwrap_or("Group")
    } else if outgoing {
        &conversation
    } else {
        e["sourceName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&conversation)
    }
    .to_owned();
    let aliases = if group_id.is_some() {
        vec![conversation.clone()]
    } else {
        let fields = if outgoing {
            ["destinationUuid", "destinationNumber", "destination"]
        } else {
            ["sourceUuid", "sourceNumber", "source"]
        };
        let object = if outgoing { sent? } else { e };
        fields
            .iter()
            .filter_map(|f| object[f].as_str().map(str::to_owned))
            .collect()
    };
    let aliases = if aliases.is_empty() {
        vec![conversation.clone()]
    } else {
        aliases
    };
    Some(DataContext {
        conversation,
        name,
        group: group_id.is_some(),
        aliases,
        sender,
        outgoing,
        data,
        edit_target: edit.and_then(|e| e["targetSentTimestamp"].as_u64()),
    })
}
pub fn quote(v: &Value) -> Option<Quote> {
    let author = v["authorUuid"]
        .as_str()
        .or(v["authorNumber"].as_str())
        .or(v["author"].as_str())?;
    Some(Quote {
        timestamp: v["id"].as_u64()?,
        author: author.to_owned(),
        body: v["text"].as_str().unwrap_or_default().to_owned(),
    })
}
pub struct Incoming {
    pub conversation: String,
    pub name: String,
    pub group: bool,
    pub body: String,
    pub outgoing: bool,
    pub timestamp: u64,
    pub sender: String,
    pub aliases: Vec<String>,
    pub quote: Option<Quote>,
    pub expires_in: u32,
    pub expiration_start: Option<u64>,
    pub attachments: Vec<Attachment>,
}
pub fn incoming(v: &Value) -> Option<Incoming> {
    let context = data_context(v)?;
    if context.edit_target.is_some() {
        return None;
    }
    let m = context.data;
    if m["viewOnce"].as_bool() == Some(true) || m["isExpirationUpdate"].as_bool() == Some(true) {
        return None;
    }
    let body = m["message"].as_str().unwrap_or_default().to_owned();
    let attachments: Vec<_> = m["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .take(32)
        .map(|a| Attachment {
            id: a["id"].as_str().unwrap_or_default().to_owned(),
            name: crate::media::safe_name(a["filename"].as_str().unwrap_or("Attachment")),
            content_type: a["contentType"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .to_owned(),
            size: a["size"].as_u64().unwrap_or(0),
            voice_note: a["isVoiceNote"].as_bool().unwrap_or(false),
            ..Default::default()
        })
        .collect();
    if body.is_empty() && attachments.is_empty() {
        return None;
    }
    Some(Incoming {
        conversation: context.conversation,
        name: context.name,
        group: context.group,
        body,
        outgoing: context.outgoing,
        timestamp: m["timestamp"]
            .as_u64()
            .or(envelope(v)?["timestamp"].as_u64())
            .unwrap_or(0),
        sender: context.sender,
        aliases: context.aliases,
        quote: quote(&m["quote"]),
        attachments,
        expires_in: m["expiresInSeconds"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        expiration_start: envelope(v)?["syncMessage"]["sentMessage"]["expirationStartTimestamp"]
            .as_u64()
            .filter(|ts| *ts > 0),
    })
}
/// Canonicalize display-formatted international numbers without guessing a country.
pub fn recipient_id(raw: &str) -> Option<String> {
    if raw.starts_with('+') {
        let number: String = raw
            .chars()
            .filter(|c| !c.is_ascii_whitespace() && !matches!(c, '-' | '(' | ')'))
            .collect();
        let digits = &number[1..];
        return (digits.len() >= 7
            && digits.len() <= 15
            && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(number);
    }
    let valid_uuid = raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    valid_uuid.then(|| raw.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn parses_numberless_account() {
        assert_eq!(
            account_id(&json!({"number":null,"aci":"abc"})),
            Some("abc".into())
        );
    }
    #[test]
    fn parses_incoming_and_subscribed_sync() {
        let m = incoming(&json!({"params":{"envelope":{"sourceUuid":"alice","sourceName":"Alice","dataMessage":{"message":"Hi","timestamp":42}}}})).unwrap();
        assert_eq!(m.conversation, "alice");
        assert!(!m.outgoing);
        let m = incoming(&json!({"params":{"result":{"envelope":{"syncMessage":{"sentMessage":{"destinationUuid":"bob","message":"Hello","timestamp":43}}}}}})).unwrap();
        assert_eq!(m.conversation, "bob");
        assert!(m.outgoing);
    }
    #[test]
    fn recipient_validation_normalizes_numbers_and_rejects_ambiguous_input() {
        assert_eq!(
            recipient_id("+1 (202) 555-0123").as_deref(),
            Some("+12025550123")
        );
        assert_eq!(
            recipient_id("11111111-1111-4111-8111-111111111111").as_deref(),
            Some("11111111-1111-4111-8111-111111111111")
        );
        assert!(recipient_id("2025550123").is_none());
        assert!(recipient_id("+123abc45678").is_none());
        assert!(recipient_id("").is_none());
    }
    #[test]
    fn ephemeral_content_is_not_retained() {
        for options in [json!({"viewOnce":true}), json!({"isExpirationUpdate":true})] {
            let mut m = json!({"message":"secret","timestamp":42});
            m.as_object_mut()
                .unwrap()
                .extend(options.as_object().unwrap().clone());
            assert!(
                incoming(&json!({"params":{"envelope":{"source":"peer","dataMessage":m}}}))
                    .is_none()
            );
        }
    }
    #[test]
    fn routes_groups_and_ignores_receipts() {
        assert!(incoming(&json!({"params":{"envelope":{"receiptMessage":{}}}})).is_none());
        let m = incoming(&json!({"params":{"envelope":{"source":"alice","dataMessage":{"message":"Hi","groupInfo":{"groupId":"g"}}}}})).unwrap();
        assert!(m.group);
        assert_eq!(m.conversation, "g");
    }
}
