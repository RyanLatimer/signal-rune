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
    pub local_path: Option<String>,
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
    pub quote: Option<Quote>,
    pub attachments: Vec<Attachment>,
    pub reactions: BTreeMap<String, String>,
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
    pub aliases: Vec<String>,
    pub messages: Vec<Message>,
    pub unread: u32,
    pub draft: String,
    pub pinned: u64,
    pub archived: bool,
    pub blocked: bool,
    pub muted_until: Option<u64>,
    pub expiration: u32,
    pub members: Vec<String>,
    #[serde(skip)]
    pub typing: HashMap<String, TypingPresence>,
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
    pub fn matches(&self, id: &str) -> bool {
        self.id == id || self.aliases.iter().any(|a| a == id)
    }
    pub fn recount_unread(&mut self) {
        self.unread = self
            .messages
            .iter()
            .filter(|m| !m.outgoing && !m.read)
            .count() as u32;
    }
    pub fn sort_messages(&mut self) {
        self.messages.sort_by_key(|m| (m.timestamp, m.id));
    }
    pub fn apply_receipt(&mut self, sender: &str, timestamps: &[u64], delivery: Delivery) -> bool {
        let mut changed = false;
        for m in &mut self.messages {
            if m.outgoing && timestamps.contains(&m.timestamp) {
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
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            dark: true,
            typing_indicators: true,
            read_receipts: true,
            notifications: true,
            last_outgoing_timestamp: 0,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncLedger {
    pub receipts: BTreeMap<String, BTreeMap<u64, Delivery>>,
    pub read_through: BTreeMap<String, u64>,
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
pub struct Incoming {
    pub conversation: String,
    pub name: String,
    pub group: bool,
    pub body: String,
    pub outgoing: bool,
    pub timestamp: u64,
    pub sender: String,
    pub aliases: Vec<String>,
}
pub fn incoming(v: &Value) -> Option<Incoming> {
    let p = &v["params"];
    let e = p
        .get("envelope")
        .or_else(|| p.get("result").and_then(|r| r.get("envelope")))?;
    let (m, outgoing) = if let Some(m) = e.get("dataMessage") {
        (m, false)
    } else {
        (e.get("syncMessage")?.get("sentMessage")?, true)
    };
    // Never retain content whose disappearing/view-once semantics this build cannot honor.
    if m["viewOnce"].as_bool() == Some(true) || m["expiresInSeconds"].as_u64().unwrap_or(0) > 0 {
        return None;
    }
    let body = m["message"].as_str()?.to_string();
    if body.is_empty() {
        return None;
    }
    let group_id = m["groupInfo"]["groupId"]
        .as_str()
        .or(m["groupV2"]["id"].as_str());
    let peer = if outgoing {
        m["destinationUuid"]
            .as_str()
            .or(m["destinationNumber"].as_str())
            .or(m["destination"].as_str())
    } else {
        e["sourceUuid"]
            .as_str()
            .or(e["sourceNumber"].as_str())
            .or(e["source"].as_str())
    };
    let conversation = group_id.or(peer)?.to_string();
    let name = if group_id.is_some() {
        m["groupInfo"]["groupName"]
            .as_str()
            .or(m["groupInfo"]["name"].as_str())
            .unwrap_or("Group")
    } else if outgoing {
        &conversation
    } else {
        e["sourceName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&conversation)
    }
    .to_string();
    Some(Incoming {
        conversation: conversation.clone(),
        name,
        group: group_id.is_some(),
        body,
        sender: source(e).unwrap_or_default().to_owned(),
        aliases: if group_id.is_some() {
            vec![conversation.clone()]
        } else {
            let fields = if outgoing {
                ["destinationUuid", "destinationNumber", "destination"]
            } else {
                ["sourceUuid", "sourceNumber", "source"]
            };
            let obj = if outgoing { m } else { e };
            fields
                .iter()
                .filter_map(|f| obj[f].as_str().map(str::to_owned))
                .collect()
        },
        outgoing,
        timestamp: m["timestamp"]
            .as_u64()
            .or(e["timestamp"].as_u64())
            .unwrap_or(0),
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
        for options in [json!({"viewOnce":true}), json!({"expiresInSeconds":30})] {
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
