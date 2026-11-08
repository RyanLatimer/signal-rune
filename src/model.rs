use serde_json::Value;
#[derive(Clone, Debug, PartialEq)]
pub enum Delivery {
    Pending,
    Sent,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Message {
    pub id: u64,
    pub body: String,
    pub outgoing: bool,
    pub timestamp: u64,
    pub delivery: Delivery,
}
#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub name: String,
    pub group: bool,
    pub messages: Vec<Message>,
    pub unread: u32,
    pub draft: String,
}
impl Conversation {
    pub fn new(id: String, name: String, group: bool) -> Self {
        Self {
            id,
            name,
            group,
            messages: vec![],
            unread: 0,
            draft: String::new(),
        }
    }
}
pub fn account_id(v: &Value) -> Option<String> {
    v["number"]
        .as_str()
        .or(v["aci"].as_str())
        .map(str::to_owned)
}
pub struct Incoming {
    pub conversation: String,
    pub name: String,
    pub group: bool,
    pub body: String,
    pub outgoing: bool,
    pub timestamp: u64,
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
        m["destinationNumber"]
            .as_str()
            .or(m["destinationUuid"].as_str())
            .or(m["destination"].as_str())
    } else {
        e["sourceNumber"]
            .as_str()
            .or(e["sourceUuid"].as_str())
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
        conversation,
        name,
        group: group_id.is_some(),
        body,
        outgoing,
        timestamp: m["timestamp"]
            .as_u64()
            .or(e["timestamp"].as_u64())
            .unwrap_or(0),
    })
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
    fn routes_groups_and_ignores_receipts() {
        assert!(incoming(&json!({"params":{"envelope":{"receiptMessage":{}}}})).is_none());
        let m = incoming(&json!({"params":{"envelope":{"source":"alice","dataMessage":{"message":"Hi","groupInfo":{"groupId":"g"}}}}})).unwrap();
        assert!(m.group);
        assert_eq!(m.conversation, "g");
    }
}
