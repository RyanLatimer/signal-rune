mod actions;
mod conversations;
mod expiration;
mod media;
mod model;
mod pins;
mod storage;
mod sync;
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
    Subscribe,
    Configuration,
    Transient,
    Receipt(String, Vec<(u64, u64)>),
    Send(String, u64),
    Action(u64),
    Identities(String),
    VerifyIdentity(String),
    ConversationUpdate(String, model::ConversationChange),
    GroupUpdate(String),
    CreatedGroup,
    JoinedGroup,
    Pin(u64),
    PinSync,
}
pub struct State {
    bridge: Option<transport::Bridge>,
    wake: Option<rune::Waker>,
    pending: HashMap<u64, Pending>,
    serial: u64,
    account: Option<String>,
    account_aliases: Vec<String>,
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
    store: Option<storage::Worker>,
    history_ready: bool,
    history_protection: String,
    generation: u64,
    saved_generation: u64,
    dirty: bool,
    preferences: model::Preferences,
    sync_ledger: model::SyncLedger,
    clock: f64,
    typing_outgoing: Option<sync::OutgoingTyping>,
    window_focused: bool,
    messages_at_bottom: bool,
    show_archived: bool,
    receipt_retry_at: f64,
    reconnect_attempt: u32,
    reconnect_at: Option<f64>,
    draft_save_at: Option<f64>,
    draft_dirty: bool,
    engine_configuration: Option<model::EngineConfiguration>,
    configuration_due: f64,
    configuration_error: Option<String>,
    reply_to: Option<model::Quote>,
    editing: Option<u64>,
    edit_backup: Option<model::DraftBackup>,
    message_context: Option<actions::MessageContext>,
    reaction_target: Option<(String, u64)>,
    message_details: Option<(String, u64)>,
    delete_confirmation: Option<(String, u64, bool)>,
    media_tasks: HashMap<u64, media::Task>,
    media_failures: HashMap<String, String>,
    media_cache: Option<rune::ImageCache>,
    media_images: HashMap<String, rune::Image>,
    conversation_info: Option<String>,
    conversation_name: TextEdit,
    identity_panel: Option<model::IdentityPanel>,
    group_member_input: TextEdit,
    group_description_input: TextEdit,
    group_member_panel: Option<(String, String)>,
    group_confirmation: Option<(String, model::GroupOperation)>,
    new_group: bool,
    group_name_input: TextEdit,
    group_participants: Vec<String>,
    pin_state: Option<model::PinState>,
    pin_accepted: std::collections::HashSet<u64>,
    pin_retry_due: f64,
    pin_sync_error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            bridge: None,
            wake: None,
            pending: HashMap::new(),
            serial: 0,
            account: None,
            account_aliases: vec![],
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
            store: None,
            history_ready: false,
            history_protection: "Opening history…".into(),
            generation: 0,
            saved_generation: 0,
            dirty: false,
            preferences: model::Preferences::default(),
            sync_ledger: model::SyncLedger::default(),
            clock: 0.,
            typing_outgoing: None,
            window_focused: false,
            messages_at_bottom: true,
            show_archived: false,
            receipt_retry_at: 0.,
            reconnect_attempt: 0,
            reconnect_at: None,
            draft_save_at: None,
            draft_dirty: false,
            engine_configuration: None,
            configuration_due: 0.,
            configuration_error: None,
            reply_to: None,
            editing: None,
            edit_backup: None,
            message_context: None,
            reaction_target: None,
            message_details: None,
            delete_confirmation: None,
            media_tasks: HashMap::new(),
            media_failures: HashMap::new(),
            media_cache: None,
            media_images: HashMap::new(),
            conversation_info: None,
            conversation_name: TextEdit::new(""),
            identity_panel: None,
            group_member_input: TextEdit::new(""),
            group_description_input: TextEdit::new(""),
            group_member_panel: None,
            group_confirmation: None,
            new_group: false,
            group_name_input: TextEdit::new(""),
            group_participants: vec![],
            pin_state: None,
            pin_accepted: std::collections::HashSet::new(),
            pin_retry_due: 0.,
            pin_sync_error: None,
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
        self.pin_accepted.clear();
        self.pin_retry_due = 0.;
        if let Some(panel) = &mut self.identity_panel {
            panel.loading = false;
            panel.saving = false;
            panel.error =
                Some("Reconnecting. Refresh the safety number when Signal is connected.".into());
        }
        self.stop_typing();
        self.engine_configuration = None;
        self.bridge = None;
        self.pending.clear();
        self.reconnect_at = None;
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
                self.schedule_reconnect();
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
        let same_identity = self.account.as_ref() == Some(&account);
        let same_account = same_identity && self.history_ready;
        self.account = Some(account.clone());
        self.qr = None;
        self.linking = false;
        if same_account {
            self.begin_sync();
            return;
        }
        self.store = None;
        self.history_ready = false;
        if !same_identity {
            self.conversations.clear();
            self.sync_ledger = model::SyncLedger::default();
            self.selected = None;
            self.composer.clear();
        }
        self.status = "Opening encrypted history…".into();
        if let Some(wake) = self.wake.clone() {
            match storage::Worker::start(account, wake) {
                Ok(worker) => self.store = Some(worker),
                Err(e) => self.error = Some(e),
            }
        }
    }
    fn choose(&mut self, id: String) {
        self.stop_typing();
        self.message_context = None;
        self.remember_draft();
        if let Some(old) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
        {
            old.draft = self.composer.text.clone();
        }
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == id) {
            c.active = true;
            self.composer.replace_text(c.draft.clone());
            self.reply_to = c.draft_quote.clone();
            self.editing = c.draft_edit;
            self.edit_backup = c.draft_backup.clone();
        }
        self.selected = Some(id);
        self.composer.focused = true;
        self.messages_at_bottom = true;
        self.mark_selected_read();
        self.dirty = true;
    }
    fn new_conversation(&mut self) {
        let raw = self.recipient.text.trim();
        if raw.starts_with("https://signal.group/#") {
            let uri = raw.to_owned();
            self.request("joinGroup", json!({"uri":uri}), Pending::JoinedGroup);
            return;
        }
        let Some(id) = model::recipient_id(raw).or_else(|| {
            self.conversations
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(raw))
                .map(|c| c.id.clone())
        }) else {
            self.error = Some("Enter a valid international phone number or account UUID.".into());
            return;
        };
        let id = self.resolve_id(&id);
        if !self.conversations.iter().any(|c| c.id == id) {
            self.conversations
                .insert(0, Conversation::new(id.clone(), id.clone(), false));
        }
        self.choose(id);
        self.new_chat = false;
        self.recipient.clear();
    }
    fn send(&mut self, tx: &mut Tx) {
        if self.editing.is_some() {
            self.submit_edit();
            return;
        }
        let body = self.composer.text.trim().to_string();
        let has_attachments = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter().find(|c| &c.id == id))
            .is_some_and(|c| !c.draft_attachments.is_empty());
        if (body.is_empty() && !has_attachments) || self.demo {
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        if !self.history_ready || self.store.is_none() {
            self.error = Some(
                "Encrypted history must be available before sending. Your draft has been kept."
                    .into(),
            );
            return;
        }
        self.stop_typing();
        let timestamp =
            model::now_ms().max(self.preferences.last_outgoing_timestamp.saturating_add(1));
        self.preferences.last_outgoing_timestamp = timestamp;
        let Some(c) = self.conversations.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if c.blocked {
            self.error = Some("Unblock this conversation before sending.".into());
            return;
        }
        self.preferences.next_message_id += 1;
        c.messages.push(Message {
            id: self.preferences.next_message_id,
            timestamp,
            body,
            outgoing: true,
            sender: self.account.clone().unwrap_or_default(),
            read: true,
            delivery: Delivery::Pending,
            quote: self.reply_to.take(),
            expires_in: c.expiration,
            attachments: std::mem::take(&mut c.draft_attachments),
            ..Message::default()
        });
        c.draft.clear();
        c.draft_quote = None;
        c.archived = false;
        self.composer.clear();
        self.dirty = true;
        // The outbox is sent only after this generation commits to encrypted history.
        self.persist();
        tx.scroll_to_bottom("messages");
    }
    fn composer_key(&mut self, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
        if self.composer.preedit.is_some() {
            return false;
        }
        match ev.key {
            rune::Key::Enter if !ev.mods.shift => {
                self.send(tx);
                true
            }
            rune::Key::Up if self.composer.text.is_empty() => {
                self.edit_last_message();
                true
            }
            _ => false,
        }
    }
    fn event(&mut self, v: Value, tx: &mut Tx) {
        if let Some(id) = v["id"].as_u64() {
            let Some(p) = self.pending.remove(&id) else {
                return;
            };
            if v.get("error").is_some() {
                if matches!(p, Pending::Pin(_) | Pending::PinSync) {
                    self.pin_sync_error=Some("Pin sync is waiting for the bundled Signal engine and a connection. Local pins have been kept.".into());
                    self.pin_retry_due = self.clock + 30.;
                    tx.wake_after(std::time::Duration::from_secs(30));
                    return;
                }
                if self.identity_error(&p) {
                    return;
                }
                // Do not put server payloads (which can contain message text/keys) into logs or screenshots.
                if matches!(p, Pending::Configuration) {
                    self.engine_configuration = None;
                    self.configuration_error = Some("Phone privacy settings could not be read. Use the bundled engine; typing is paused until its settings are available.".into());
                    self.stop_typing();
                }
                if !matches!(
                    p,
                    Pending::Configuration | Pending::Transient | Pending::Receipt(..)
                ) {
                    self.error = Some(format!(
                        "Signal request failed (code {}). Check your connection and try again.",
                        v["error"]["code"]
                    ));
                }
                if matches!(p, Pending::Receipt(..)) {
                    self.receipt_retry_at = self.clock + 30.;
                    tx.wake_after(std::time::Duration::from_secs(30));
                }
                if let Pending::Action(action) = &p
                    && let Some(a) = self
                        .sync_ledger
                        .actions
                        .iter_mut()
                        .find(|a| a.id == *action)
                {
                    a.failed = true;
                    self.dirty = true;
                }
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
                Pending::Identities(peer) => self.identity_result(&peer, r),
                Pending::VerifyIdentity(peer) => self.request_identity(&peer),
                Pending::ConversationUpdate(peer, change) => {
                    self.conversation_updated(&peer, change)
                }
                Pending::GroupUpdate(peer) => self.group_updated(&peer, r),
                Pending::Pin(id) => self.pin_result(id, r),
                Pending::PinSync => {}
                Pending::CreatedGroup | Pending::JoinedGroup => self.group_joined(r),
                Pending::Accounts => {
                    if let Some(account) = r
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(model::account_id)
                    {
                        self.account_aliases = ["number", "aci"]
                            .iter()
                            .filter_map(|f| r[0][f].as_str().map(str::to_owned))
                            .collect();
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
                        self.account_aliases = ["number", "aci"]
                            .iter()
                            .filter_map(|f| r[f].as_str().map(str::to_owned))
                            .collect();
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
                    if let Some(items) = r.as_array() {
                        self.merge_contacts(items, matches!(p, Pending::Groups));
                    }
                }
                Pending::Action(action) => self.action_finished(action, r),
                Pending::Sync => {}
                Pending::Transient => {
                    if r["privacySuppressed"] == true {
                        self.configuration_due = self.clock;
                    }
                }
                Pending::Configuration => {
                    self.receive_pin_state(&r["conversationPins"]);
                    match serde_json::from_value::<model::EngineConfiguration>(r.clone()) {
                        Ok(configuration) if configuration.version == 1 => {
                            if !configuration.typing_indicators {
                                self.stop_typing();
                                for c in &mut self.conversations {
                                    c.typing.clear();
                                }
                            }
                            self.engine_configuration = Some(configuration);
                            self.configuration_error = None;
                        }
                        _ => {
                            self.engine_configuration = None;
                            self.stop_typing();
                            self.configuration_error = Some("The engine returned unsupported phone privacy settings. Typing is paused.".into());
                        }
                    }
                }
                Pending::Subscribe => {
                    self.status = "Connected".into();
                    self.reconnect_attempt = 0;
                    self.reconnect_at = None;
                    self.error = None;
                    self.flush_outbox();
                }
                Pending::Receipt(peer, ids) => {
                    if r["results"].as_array().is_some_and(|a| {
                        a.iter()
                            .any(|x| x["type"].as_str().is_some_and(|t| t != "SUCCESS"))
                    }) {
                        self.receipt_retry_at = self.clock + 30.;
                        tx.wake_after(std::time::Duration::from_secs(30));
                        return;
                    }
                    if let Some(c) = self.conversations.iter_mut().find(|c| c.id == peer) {
                        for m in &mut c.messages {
                            if ids.contains(&(m.id, m.latest_timestamp())) {
                                m.read_receipt_sent = true;
                            }
                        }
                    }
                    self.dirty = true;
                }
            }
        } else if v["method"] == "receive" {
            self.receive(&v, tx);
        }
        self.persist();
    }
    fn delivery(&mut self, peer: &str, id: u64, d: Delivery, timestamp: u64) {
        if let Some(m) = self
            .conversations
            .iter_mut()
            .find(|c| c.id == peer)
            .and_then(|c| c.messages.iter_mut().find(|m| m.id == id))
        {
            if d == Delivery::Failed && !matches!(m.delivery, Delivery::Delivered | Delivery::Read)
            {
                m.delivery = d;
            } else {
                m.delivery.advance(d);
            }
            if timestamp != 0 {
                m.timestamp = timestamp;
            }
            if matches!(d, Delivery::Sent | Delivery::Delivered | Delivery::Read) {
                m.start_expiration(model::now_ms());
            }
            self.dirty = true;
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if self.draft_dirty {
            self.dirty = true;
        }
        self.persist();
    }
}
fn setup(s: &mut State, wake: rune::Waker) {
    s.media_cache = Some(rune::ImageCache::new(wake.clone()));
    s.wake = Some(wake);
    // Snapshot runs must never touch the user's account, start network traffic or consume messages.
    if rune::tooling::headless_run().is_none() {
        s.connect();
    }
}
fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    s.clock = now;
    let mut changed = s.poll_history(tx) | s.poll_timers(tx);
    loop {
        let event = s.bridge.as_ref().and_then(|b| b.events.try_recv().ok());
        match event {
            Some(transport::Event::Json(v)) => {
                s.event(v, tx);
                changed = true;
            }
            Some(transport::Event::Stopped) => {
                s.pin_accepted.clear();
                s.pin_retry_due = 0.;
                if let Some(panel) = &mut s.identity_panel {
                    panel.loading = false;
                    panel.saving = false;
                    panel.error = Some(
                        "Signal disconnected. Refresh the safety number after reconnecting.".into(),
                    );
                }
                s.status = "Disconnected".into();
                s.error = Some("Signal engine stopped. Retry connection in Settings.".into());
                s.bridge = None;
                s.qr = None;
                s.linking = false;
                s.pending.clear();
                s.typing_outgoing = None;
                s.schedule_reconnect();
                if let Some(at) = s.reconnect_at {
                    tx.wake_after(std::time::Duration::from_secs_f64(
                        (at - s.clock).max(0.001),
                    ));
                }
                return true;
            }
            None => break,
        }
    }
    if s.dirty {
        s.persist();
    }
    changed
}
fn on_key(s: &mut State, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        rune::Key::Escape => {
            if s.new_group {
                s.new_group = false;
                return true;
            }
            if s.group_confirmation.take().is_some() || s.group_member_panel.take().is_some() {
                return true;
            }
            if s.identity_panel.take().is_some() || s.conversation_info.take().is_some() {
                return true;
            }
            if s.message_context.take().is_some()
                || s.reaction_target.take().is_some()
                || s.message_details.take().is_some()
                || s.delete_confirmation.take().is_some()
            {
                return true;
            }
            if s.editing.is_some() {
                s.cancel_edit();
            } else if s.reply_to.take().is_some() {
                s.dirty = true;
                s.persist();
            } else {
                s.new_chat = false;
                s.settings = false;
            }
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
    rune::App::new(AppSpec {
        on_key: Some(on_key),
        setup: Some(setup),
        poll: Some(poll),
        min_size: Some((760.0, 560.0)),
        scenes: ui::SCENES,
        fonts: &[
            include_bytes!("../assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../assets/fonts/Inter-Medium.ttf"),
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
            include_bytes!("../assets/fonts/NotoColorEmoji.ttf"),
        ],
        families: ("Inter, Noto Color Emoji", "Inter, Noto Color Emoji"),
        ..AppSpec::new("Signal Rune", (1120.0, 760.0), State::default, ui::view)
    })
    .on_focus(|s, focused, _| {
        s.window_focused = focused;
        if !focused {
            s.stop_typing();
        } else {
            s.mark_selected_read();
            s.persist();
        }
    })
    .run();
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
        s.window_focused = true;
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
    fn composer_shift_enter_and_ime_do_not_send() {
        let mut s = fixture();
        s.composer.replace_text("draft");
        let key = |shift| rune::KeyEvent {
            key: rune::Key::Enter,
            mods: rune::Mods {
                shift,
                ..Default::default()
            },
        };
        assert!(!s.composer_key(&key(true), &mut Tx::default()));
        s.composer.preedit = Some(("composing".into(), None));
        assert!(!s.composer_key(&key(false), &mut Tx::default()));
        assert_eq!(s.composer.text, "draft");
    }
    #[test]
    fn native_composer_grows_and_keeps_multiline_draft() {
        let mut s = fixture();
        s.composer.focused = true;
        s.composer.replace_text("first");
        let mut rt = Runtime::new(s, ui::view, (760., 560.), Env::default());
        for _ in 0..60 {
            rt.frame(1. / 60.);
        }
        let first = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .unwrap()
            .target[3];
        rt.key(rune::KeyEvent {
            key: rune::Key::Enter,
            mods: rune::Mods {
                shift: true,
                ..Default::default()
            },
        });
        rt.ime(rune::Ime::Commit("second".into()));
        for _ in 0..60 {
            rt.frame(1. / 60.);
        }
        assert_eq!(rt.state.composer.text, "first\nsecond");
        let second = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("composer"))
            .unwrap()
            .target[3];
        assert!(second > first);
        rt.state.choose("demo-1".into());
        rt.state.choose("demo-0".into());
        assert_eq!(rt.state.composer.text, "first\nsecond");
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
            ..Message::default()
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
        assert!(!s.error.as_ref().unwrap().contains("sensitive payload"));
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
    #[allow(
        clippy::field_reassign_with_default,
        reason = "State has a flushing Drop implementation and cannot use struct update syntax"
    )]
    fn link_expiry_clears_qr_and_allows_retry() {
        let mut s = State::default();
        s.linking = true;
        s.qr = qrcode::QrCode::new("fixture").ok();
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
    fn native_message_menu_starts_reply() {
        let mut rt = Runtime::new(fixture(), ui::view, (1120., 760.), Env::default());
        for _ in 0..60 {
            rt.frame(1. / 60.);
        }
        let (x, y) = rt.find("message-more-6").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1. / 60.);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..30 {
            rt.frame(1. / 60.);
        }
        assert!(rt.state.message_context.is_some());
        let (x, y) = rt.find("message-menu-6-item0").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1. / 60.);
        assert!(rt.state.reply_to.is_some());
        assert!(rt.find("compose-context").is_some());
    }
    #[test]
    fn native_group_details_can_scroll_to_every_control_at_minimum_size() {
        let mut s = State::default();
        ui::SCENES
            .iter()
            .find(|(name, _)| *name == "group-details")
            .unwrap()
            .1(&mut s);
        let mut rt = Runtime::new(s, ui::view, (760., 560.), Env::default());
        for _ in 0..60 {
            rt.frame(1. / 60.);
        }
        rt.update(|_, tx| tx.scroll_into_view("block-conversation"));
        for _ in 0..120 {
            rt.frame(1. / 60.);
        }
        let target = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("block-conversation"))
            .unwrap();
        assert!(
            target.visible[3] > 20.,
            "Group controls must be reachable by scrolling: {:?}",
            target.visible
        );
        assert!(target.rect[1] >= 0. && target.rect[1] + target.rect[3] <= 560.);
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
