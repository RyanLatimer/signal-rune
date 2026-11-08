mod model;
mod transport;
mod ui;
use model::{Conversation, Delivery, Message};
use rune::{AppSpec, TextEdit, Tx};
use serde_json::{Value, json};
use std::collections::HashMap;

enum Pending {
    Accounts,
    Link,
    Finish,
    Contacts,
    Groups,
    Sync,
    Send(String, u64),
}
pub struct State {
    bridge: Option<transport::Bridge>,
    wake: Option<rune::Waker>,
    pending: HashMap<u64, Pending>,
    serial: u64,
    account: Option<String>,
    status: String,
    error: Option<String>,
    qr: Option<qrcode::QrCode>,
    linking: bool,
    conversations: Vec<Conversation>,
    selected: Option<String>,
    search: TextEdit,
    composer: TextEdit,
    recipient: TextEdit,
    dark: bool,
    settings: bool,
    new_chat: bool,
    demo: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            bridge: None,
            wake: None,
            pending: HashMap::new(),
            serial: 0,
            account: None,
            status: "Connecting to Signal engine…".into(),
            error: None,
            qr: None,
            linking: false,
            conversations: vec![],
            selected: None,
            search: TextEdit::new(""),
            composer: TextEdit::new(""),
            recipient: TextEdit::new(""),
            dark: true,
            settings: false,
            new_chat: false,
            demo: false,
        }
    }
}
impl State {
    fn request(&mut self, method: &str, mut params: Value, pending: Pending) -> Option<u64> {
        if self.demo {
            return None;
        }
        if !matches!(pending, Pending::Accounts | Pending::Link | Pending::Finish) {
            params["account"] = json!(self.account.as_ref()?);
        }
        self.serial += 1;
        let id = self.serial;
        let result = self
            .bridge
            .as_ref()
            .ok_or("Signal engine is offline".to_string())
            .and_then(|b| b.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})));
        if let Err(e) = result {
            self.error = Some(e);
            return None;
        }
        self.pending.insert(id, pending);
        Some(id)
    }
    fn connect(&mut self) {
        self.bridge = None;
        self.pending.clear();
        self.linking = false;
        self.qr = None;
        let Some(wake) = self.wake.clone() else {
            return;
        };
        match transport::Bridge::start(wake) {
            Ok(bridge) => {
                self.bridge = Some(bridge);
                self.error = None;
                self.status = "Connecting…".into();
                self.request("listAccounts", json!({}), Pending::Accounts);
            }
            Err(e) => {
                self.status = "Engine unavailable".into();
                self.error = Some(e);
            }
        }
    }
    fn start_link(&mut self) {
        if self.linking || self.bridge.is_none() {
            return;
        }
        self.error = None;
        self.linking = true;
        self.qr = None;
        self.status = "Creating a secure link…".into();
        self.request("startLink", json!({}), Pending::Link);
    }
    fn linked(&mut self, account: String) {
        self.account = Some(account);
        self.qr = None;
        self.linking = false;
        self.status = "Connected".into();
        self.request("listContacts", json!({}), Pending::Contacts);
        self.request("listGroups", json!({}), Pending::Groups);
        self.request("sendSyncRequest", json!({}), Pending::Sync);
    }
    fn choose(&mut self, id: String) {
        if let Some(old) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
        {
            old.draft = self.composer.text.clone();
        }
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == id) {
            c.unread = 0;
            self.composer.replace_text(c.draft.clone());
        }
        self.selected = Some(id);
        self.composer.focused = true;
    }
    fn new_conversation(&mut self) {
        let id = self.recipient.text.trim().to_string();
        if id.is_empty() {
            return;
        }
        if !self.conversations.iter().any(|c| c.id == id) {
            self.conversations
                .insert(0, Conversation::new(id.clone(), id.clone(), false));
        }
        self.choose(id);
        self.new_chat = false;
        self.recipient.clear();
    }
    fn send(&mut self, tx: &mut Tx) {
        let body = self.composer.text.trim().to_string();
        if body.is_empty() || self.demo {
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(c) = self.conversations.iter().find(|c| c.id == id) else {
            return;
        };
        let mut params = json!({"message":body});
        params[if c.group { "groupId" } else { "recipient" }] = json!(id);
        // A pending row appears only after the request was accepted by the transport.
        let local_id = self.serial + 1;
        if self
            .request("send", params, Pending::Send(id.clone(), local_id))
            .is_some()
        {
            let c = self.conversations.iter_mut().find(|c| c.id == id).unwrap();
            c.messages.push(Message {
                id: local_id,
                body,
                outgoing: true,
                timestamp: 0,
                delivery: Delivery::Pending,
            });
            c.draft.clear();
            self.composer.clear();
            tx.scroll_to_bottom("messages");
        }
    }
    fn event(&mut self, v: Value, tx: &mut Tx) {
        if let Some(id) = v["id"].as_u64() {
            let Some(p) = self.pending.remove(&id) else {
                return;
            };
            if v.get("error").is_some() {
                // Do not put server payloads (which can contain message text/keys) into logs or screenshots.
                self.error = Some(format!(
                    "Signal request failed (code {}). Check your connection and try again.",
                    v["error"]["code"]
                ));
                if let Pending::Send(peer, mid) = p {
                    self.delivery(&peer, mid, Delivery::Failed, 0);
                } else if matches!(p, Pending::Link | Pending::Finish) {
                    self.linking = false;
                    self.qr = None;
                    self.status = "Link expired or declined. Try again.".into();
                }
                return;
            }
            let r = &v["result"];
            match p {
                Pending::Accounts => {
                    if let Some(account) = r
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(model::account_id)
                    {
                        self.linked(account);
                    } else {
                        self.status = "Ready to link your phone".into();
                    }
                }
                Pending::Link => {
                    if let Some(uri) = r["deviceLinkUri"]
                        .as_str()
                        .filter(|u| u.starts_with("sgnl://linkdevice?"))
                    {
                        match qrcode::QrCode::new(uri) {
                            Ok(qr) => {
                                self.qr = Some(qr);
                                self.status = "Scan with Signal on your phone".into();
                                self.request(
                                    "finishLink",
                                    json!({"deviceLinkUri":uri,"deviceName":"Signal Rune"}),
                                    Pending::Finish,
                                );
                            }
                            Err(_) => {
                                self.linking = false;
                                self.error = Some("Could not generate the linking QR code".into());
                            }
                        }
                    } else {
                        self.linking = false;
                        self.error =
                            Some("Signal engine returned an invalid linking response".into());
                    }
                }
                Pending::Finish => {
                    if let Some(account) = model::account_id(r) {
                        self.linked(account);
                    } else {
                        self.linking = false;
                        self.qr = None;
                        self.error = Some("Link response did not contain an account".into());
                    }
                }
                Pending::Send(peer, mid) => {
                    let failed = r["results"].as_array().is_some_and(|a| {
                        a.iter().any(|x| {
                            x.get("error").is_some()
                                || x["type"].as_str().is_some_and(|t| t != "SUCCESS")
                        })
                    });
                    self.delivery(
                        &peer,
                        mid,
                        if failed {
                            Delivery::Failed
                        } else {
                            Delivery::Sent
                        },
                        r["timestamp"].as_u64().unwrap_or(0),
                    );
                    if failed {
                        self.error = Some(
                            "Message could not be delivered. Select its text to copy and retry."
                                .into(),
                        );
                    }
                }
                Pending::Contacts | Pending::Groups => {
                    let group = matches!(p, Pending::Groups);
                    if let Some(items) = r.as_array() {
                        for item in items {
                            let id = if group {
                                item["id"].as_str().map(str::to_string)
                            } else {
                                model::account_id(item)
                                    .or_else(|| item["uuid"].as_str().map(str::to_string))
                            };
                            if let Some(id) = id {
                                let name = item["name"]
                                    .as_str()
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or(&id)
                                    .to_string();
                                if let Some(c) = self.conversations.iter_mut().find(|c| c.id == id)
                                {
                                    c.name = name;
                                } else {
                                    self.conversations.push(Conversation::new(id, name, group));
                                }
                            }
                        }
                    }
                }
                Pending::Sync => {}
            }
        } else if v["method"] == "receive"
            && let Some(m) = model::incoming(&v)
        {
            let index = self
                .conversations
                .iter()
                .position(|c| c.id == m.conversation)
                .unwrap_or_else(|| {
                    self.conversations.push(Conversation::new(
                        m.conversation.clone(),
                        m.name.clone(),
                        m.group,
                    ));
                    self.conversations.len() - 1
                });
            let c = &mut self.conversations[index];
            if c.messages
                .iter()
                .any(|x| x.timestamp == m.timestamp && x.outgoing == m.outgoing && x.body == m.body)
            {
                return;
            }
            c.messages.push(Message {
                id: m.timestamp,
                body: m.body,
                outgoing: m.outgoing,
                timestamp: m.timestamp,
                delivery: Delivery::Sent,
            });
            if self.selected.as_ref() != Some(&c.id) && !m.outgoing {
                c.unread += 1;
            } else {
                tx.scroll_to_bottom("messages");
            }
        }
    }
    fn delivery(&mut self, peer: &str, id: u64, d: Delivery, timestamp: u64) {
        if let Some(m) = self
            .conversations
            .iter_mut()
            .find(|c| c.id == peer)
            .and_then(|c| c.messages.iter_mut().find(|m| m.id == id))
        {
            m.delivery = d;
            m.timestamp = timestamp;
        }
    }
}
fn setup(s: &mut State, wake: rune::Waker) {
    s.wake = Some(wake);
    // Snapshot runs must never touch the user's account, start network traffic or consume messages.
    if rune::tooling::headless_run().is_none() {
        s.connect();
    }
}
fn poll(s: &mut State, _: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    loop {
        let event = s.bridge.as_ref().and_then(|b| b.events.try_recv().ok());
        match event {
            Some(transport::Event::Json(v)) => {
                s.event(v, tx);
                changed = true;
            }
            Some(transport::Event::Stopped) => {
                s.status = "Disconnected".into();
                s.error = Some("Signal engine stopped. Retry connection in Settings.".into());
                s.bridge = None;
                s.qr = None;
                s.linking = false;
                let pending = std::mem::take(&mut s.pending);
                for (_, p) in pending {
                    if let Pending::Send(peer, mid) = p {
                        s.delivery(&peer, mid, Delivery::Failed, 0);
                    }
                }
                return true;
            }
            None => break,
        }
    }
    changed
}
fn on_key(s: &mut State, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        rune::Key::Escape => {
            s.new_chat = false;
            s.settings = false;
        }
        rune::Key::Command('n') if s.account.is_some() => {
            s.new_chat = true;
            s.recipient.focused = true;
        }
        rune::Key::Command('f') if s.account.is_some() => {
            s.search.focused = true;
        }
        rune::Key::Command(',') => s.settings = true,
        rune::Key::Command('q') => tx.quit(),
        _ => return false,
    }
    true
}
fn main() {
    rune::run(AppSpec {
        on_key: Some(on_key),
        setup: Some(setup),
        poll: Some(poll),
        min_size: Some((760.0, 560.0)),
        scenes: ui::SCENES,
        ..AppSpec::new("Signal Rune", (1120.0, 760.0), State::default, ui::view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn fixture() -> State {
        let mut s = State::default();
        ui::SCENES
            .iter()
            .find(|(name, _)| *name == "chat-dark")
            .unwrap()
            .1(&mut s);
        s
    }
    #[test]
    fn drafts_survive_switching_conversations() {
        let mut s = fixture();
        s.composer.replace_text("unfinished draft");
        s.choose("demo-1".into());
        assert!(s.composer.text.is_empty());
        assert_eq!(s.conversations[1].unread, 0);
        s.choose("demo-0".into());
        assert_eq!(s.composer.text, "unfinished draft");
    }
    #[test]
    fn failed_rpc_does_not_mark_message_sent() {
        let mut s = fixture();
        s.conversations[0].messages.push(Message {
            id: 999,
            body: "test".into(),
            outgoing: true,
            timestamp: 0,
            delivery: Delivery::Pending,
        });
        s.pending.insert(999, Pending::Send("demo-0".into(), 999));
        s.event(
            json!({"id":999,"error":{"code":-1,"message":"sensitive payload"}}),
            &mut Tx::default(),
        );
        assert_eq!(
            s.conversations[0].messages.last().unwrap().delivery,
            Delivery::Failed
        );
        assert!(!s.error.unwrap().contains("sensitive payload"));
    }
    #[test]
    fn send_result_failure_is_not_success() {
        let mut s = fixture();
        s.pending.insert(1, Pending::Send("demo-0".into(), 0));
        s.event(
            json!({"id":1,"result":{"timestamp":42,"results":[{"type":"IDENTITY_FAILURE"}]}}),
            &mut Tx::default(),
        );
        assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Failed);
    }
    #[test]
    fn incoming_replay_is_deduplicated_and_unread_counted_once() {
        let mut s = fixture();
        let v = json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","sourceName":"Peer","dataMessage":{"message":"hello","timestamp":123}}}});
        s.event(v.clone(), &mut Tx::default());
        s.event(v, &mut Tx::default());
        let c = s.conversations.iter().find(|c| c.id == "peer").unwrap();
        assert_eq!(c.messages.len(), 1);
        assert_eq!(c.unread, 1);
    }
    #[test]
    fn link_expiry_clears_qr_and_allows_retry() {
        let mut s = State {
            linking: true,
            qr: qrcode::QrCode::new("fixture").ok(),
            ..State::default()
        };
        s.pending.insert(1, Pending::Finish);
        s.event(json!({"id":1,"error":{"code":-1}}), &mut Tx::default());
        assert!(!s.linking);
        assert!(s.qr.is_none());
    }
    #[test]
    fn native_click_selects_conversation_and_opens_settings() {
        let mut rt = Runtime::new(fixture(), ui::view, (1120., 760.), Env::default());
        rt.frame(0.);
        let (x, y) = rt.find("conversation-1").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1. / 60.);
        assert_eq!(rt.state.selected.as_deref(), Some("demo-1"));
        let (x, y) = rt.find("settings").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1. / 60.);
        assert!(rt.state.settings);
        assert!(rt.find("theme-toggle").is_some());
    }
    #[test]
    fn preview_and_offline_send_preserve_draft() {
        let mut s = fixture();
        s.composer.replace_text("hello");
        let before = s.conversations[0].messages.len();
        s.send(&mut Tx::default());
        assert_eq!(s.conversations[0].messages.len(), before);
        s.demo = false;
        s.send(&mut Tx::default());
        assert_eq!(s.composer.text, "hello");
        assert_eq!(s.conversations[0].messages.len(), before);
    }
}
