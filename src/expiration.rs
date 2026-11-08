//! Wall-clock expiry is enforced before restored history is displayed and while offline.
use crate::{
    State,
    model::{self, ActionKind},
};
use serde_json::Value;

pub fn duration(seconds: u32) -> String {
    match seconds {
        0 => "Off".into(),
        n if n % 604800 == 0 => format!("{} weeks", n / 604800),
        n if n % 86400 == 0 => format!("{} days", n / 86400),
        n if n % 3600 == 0 => format!("{} hours", n / 3600),
        n if n % 60 == 0 => format!("{} minutes", n / 60),
        n => format!("{n} seconds"),
    }
}
impl State {
    pub(crate) fn receive_expiration_update(&mut self, value: &Value) {
        let Some(context) = model::data_context(value) else {
            return;
        };
        if context.edit_target.is_some() || context.data["viewOnce"] == true {
            return;
        }
        let Some(seconds) = context.data["expiresInSeconds"].as_u64() else {
            return;
        };
        let at = context.data["timestamp"].as_u64().unwrap_or(0);
        let peer = self.resolve_id(&context.conversation);
        if !self.conversations.iter().any(|c| c.id == peer) {
            self.conversations.push(crate::Conversation::new(
                peer.clone(),
                context.name,
                context.group,
            ));
        }
        let c = self
            .conversations
            .iter_mut()
            .find(|c| c.id == peer)
            .unwrap();
        // Group state is authoritative through listGroups; older received messages must
        // never roll a newer direct-conversation timer back.
        if !c.group && at > c.expiration_updated_at {
            c.expiration = seconds.min(i32::MAX as u64) as u32;
            c.expiration_updated_at = at;
            self.dirty = true;
        }
    }
    pub(crate) fn expire_messages(&mut self, now: u64) -> bool {
        let mut expired = Vec::new();
        for c in &mut self.conversations {
            for m in &mut c.messages {
                if !m.hidden && m.expires_at.is_some_and(|at| at <= now) {
                    m.tombstone_timestamps
                        .extend(m.edit_history.iter().map(|v| v.timestamp));
                    m.hidden = true;
                    m.body.clear();
                    m.quote = None;
                    m.attachments.clear();
                    m.reactions.clear();
                    m.edit_history.clear();
                    m.read = true;
                    m.read_receipt_sent = true;
                    expired.push((c.id.clone(), m.id));
                }
            }
            if expired.iter().any(|(id, _)| id == &c.id) {
                c.recount_unread();
            }
        }
        if expired.is_empty() {
            return false;
        }
        if expired
            .iter()
            .any(|(peer, id)| self.selected.as_ref() == Some(peer) && self.editing == Some(*id))
        {
            self.cancel_edit();
        }
        self.sync_ledger
            .actions
            .retain(|a| !expired.contains(&(a.conversation.clone(), a.message)));
        self.sync_ledger.mutations.retain(|mutation| {
            !matches!(mutation.kind, ActionKind::Edit { .. })
                || !self.conversations.iter().any(|c| {
                    c.id == mutation.conversation
                        && c.messages
                            .iter()
                            .any(|m| m.hidden && m.matches_timestamp(mutation.target))
                })
        });
        self.purge_tombstone_mutations();
        self.dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conversation, Message, Quote};
    #[test]
    fn expiry_never_extends_and_removes_content_while_offline() {
        let mut s = State::default();
        let mut c = Conversation::new("peer".into(), "Peer".into(), false);
        let mut m = Message {
            id: 1,
            body: "secret".into(),
            expires_in: 30,
            quote: Some(Quote {
                body: "quoted secret".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        m.start_expiration(1000);
        m.start_expiration(2000);
        assert_eq!(m.expires_at, Some(31000));
        m.start_expiration(500);
        assert_eq!(m.expires_at, Some(30500));
        c.messages.push(m);
        s.conversations.push(c);
        assert!(!s.expire_messages(30499));
        assert!(s.expire_messages(30500));
        let m = &s.conversations[0].messages[0];
        assert!(m.hidden && m.body.is_empty() && m.quote.is_none());
        assert!(!s.expire_messages(40000));
    }
    #[test]
    fn unread_message_waits_for_read_and_phone_read_time_survives_late_delivery() {
        let mut s = State::default();
        let mut c = Conversation::new("peer".into(), "Peer".into(), false);
        c.messages.push(Message {
            id: 1,
            timestamp: 100,
            sender: "peer".into(),
            expires_in: 30,
            ..Default::default()
        });
        s.conversations.push(c);
        assert!(!s.expire_messages(100000));
        s.sync_ledger.read_through.insert("peer".into(), 100);
        s.sync_ledger
            .read_at
            .entry("peer".into())
            .or_default()
            .insert(100, 1000);
        s.apply_sync_ledger();
        assert_eq!(s.conversations[0].messages[0].expires_at, Some(31000));
        assert!(s.expire_messages(31000));
    }
}
