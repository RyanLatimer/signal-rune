//! Protocol event handling, durable outbox, and foreground/read/typing state.
use crate::{
    Pending, State,
    model::{self, Conversation, Delivery, Message, TypingPresence},
    storage,
};
use rune::Tx;
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};

pub struct OutgoingTyping {
    pub conversation: String,
    pub last_sent: f64,
    pub last_edit: f64,
}
impl State {
    pub(crate) fn persist(&mut self) {
        if self.demo || !self.history_ready || !self.dirty {
            return;
        }
        self.remember_draft();
        let Some(store) = &self.store else {
            return;
        };
        self.preferences.dark = self.dark;
        self.generation += 1;
        let snapshot = storage::Snapshot {
            conversations: self.conversations.clone(),
            preferences: self.preferences.clone(),
            selected: self.selected.clone(),
            sync: self.sync_ledger.clone(),
        };
        match store.save(self.generation, snapshot) {
            Ok(()) => {
                self.dirty = false;
                self.draft_dirty = false;
                self.draft_save_at = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.history_ready = false;
            }
        }
    }
    pub(crate) fn begin_sync(&mut self) {
        self.status = "Synchronizing…".into();
        self.request("getClientState", json!({}), Pending::Configuration);
        self.configuration_due = self.clock + 15.;
        self.request("subscribeReceive", json!({}), Pending::Subscribe);
        self.request("listContacts", json!({}), Pending::Contacts);
        self.request("listGroups", json!({}), Pending::Groups);
        self.request("sendSyncRequest", json!({}), Pending::Sync);
    }
    pub(crate) fn poll_history(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        loop {
            let event = self.store.as_ref().and_then(|s| s.events.try_recv().ok());
            match event {
                Some(storage::Event::Ready(snapshot, protection)) => {
                    let mut restored = snapshot.conversations;
                    for mut current in std::mem::take(&mut self.conversations) {
                        if self.selected.as_ref() == Some(&current.id) {
                            current.draft = self.composer.text.clone();
                        }
                        if let Some(old) = restored.iter_mut().find(|c| c.id == current.id) {
                            // A discovered contact is a placeholder, not newer conversation
                            // state. Keep restored drafts, media and pins when it is empty.
                            if !current.active
                                && current.messages.is_empty()
                                && current.draft.is_empty()
                                && current.draft_quote.is_none()
                                && current.draft_edit.is_none()
                                && current.draft_attachments.is_empty()
                                && current.pinned == 0
                                && !current.archived
                            {
                                old.aliases.extend(current.aliases);
                                old.aliases.sort();
                                old.aliases.dedup();
                                continue;
                            }
                            for m in std::mem::take(&mut old.messages) {
                                if !current
                                    .messages
                                    .iter()
                                    .any(|x| x.id == m.id && x.sender == m.sender)
                                {
                                    current.messages.push(m);
                                }
                            }
                            *old = current;
                        } else {
                            restored.push(current);
                        }
                    }
                    self.conversations = restored;
                    self.preferences = snapshot.preferences;
                    self.dark = self.preferences.dark;
                    self.selected = snapshot.selected;
                    for (peer, messages) in snapshot.sync.receipts {
                        for (timestamp, delivery) in messages {
                            self.sync_ledger
                                .receipts
                                .entry(peer.clone())
                                .or_default()
                                .entry(timestamp)
                                .or_default()
                                .advance(delivery);
                        }
                    }
                    for (peer, timestamp) in snapshot.sync.read_through {
                        self.sync_ledger
                            .read_through
                            .entry(peer)
                            .and_modify(|t| *t = (*t).max(timestamp))
                            .or_insert(timestamp);
                    }
                    for action in snapshot.sync.actions {
                        if !self.sync_ledger.actions.iter().any(|a| a.id == action.id) {
                            self.sync_ledger.actions.push(action);
                        }
                    }
                    self.sync_ledger.mutations.extend(snapshot.sync.mutations);
                    for pin in snapshot.sync.pins {
                        if !self.sync_ledger.pins.iter().any(|p| p.id == pin.id) {
                            self.sync_ledger.pins.push(pin);
                        }
                    }
                    for (sender, messages) in snapshot.sync.read_at {
                        for (timestamp, at) in messages {
                            self.sync_ledger
                                .read_at
                                .entry(sender.clone())
                                .or_default()
                                .entry(timestamp)
                                .and_modify(|old| *old = (*old).min(at))
                                .or_insert(at);
                        }
                    }
                    self.apply_mutations();
                    self.apply_sync_ledger();
                    self.expire_messages(model::now_ms());
                    self.preferences.next_message_id = self.preferences.next_message_id.max(
                        self.conversations
                            .iter()
                            .flat_map(|c| &c.messages)
                            .map(|m| m.id)
                            .max()
                            .unwrap_or(0),
                    );
                    self.history_protection = protection;
                    if let Some(c) = self
                        .selected
                        .as_ref()
                        .and_then(|id| self.conversations.iter().find(|c| &c.id == id))
                    {
                        self.composer.replace_text(c.draft.clone());
                        self.reply_to = c.draft_quote.clone();
                        self.editing = c.draft_edit;
                        self.edit_backup = c.draft_backup.clone();
                    }
                    if let Some(account) = self.account.clone()
                        && !self.conversations.iter().any(|c| c.matches(&account))
                    {
                        let mut notes = Conversation::new(account, "Note to Self".into(), false);
                        notes.aliases.extend(self.account_aliases.clone());
                        self.conversations.push(notes);
                    }
                    self.history_ready = true;
                    self.dirty = true;
                    self.mark_selected_read();
                    self.persist();
                    self.begin_sync();
                    tx.scroll_to_bottom("messages");
                    changed = true;
                }
                Some(storage::Event::Saved(generation)) => {
                    self.saved_generation = generation;
                    // Only a snapshot that includes the latest UI read/send state may release
                    // outbox sends or read receipts to the network.
                    if generation == self.generation {
                        self.flush_outbox();
                        self.flush_actions();
                        self.flush_pins();
                        self.flush_read_receipts(tx);
                    }
                }
                Some(storage::Event::Media(ticket, result)) => {
                    self.media_result(ticket, result);
                    changed = true;
                }
                Some(storage::Event::Error(error)) => {
                    self.history_ready = false;
                    self.history_protection = "History unavailable".into();
                    self.error = Some(error);
                    self.status = "History unavailable · receiving paused".into();
                    self.bridge = None;
                    self.pending.clear();
                    self.reconnect_at = None;
                    changed = true;
                }
                None => break,
            }
        }
        changed
    }
    pub(crate) fn flush_outbox(&mut self) {
        if self.dirty
            || !self.history_ready
            || self.demo
            || self.bridge.is_none()
            || self.saved_generation != self.generation
        {
            return;
        }
        let queued: Vec<_> = self
            .conversations
            .iter()
            .filter(|c| !c.blocked)
            .flat_map(|c| {
                c.messages
                    .iter()
                    .filter(|m| {
                        m.outgoing
                            && !m.hidden
                            && !m.deleted
                            && m.delivery == Delivery::Pending
                            && m.timestamp < model::now_ms()
                    })
                    .map(|m| (c.id.clone(), c.group, m.clone()))
            })
            .collect();
        for (peer, group, m) in queued {
            if self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Send(_, id) if *id == m.id))
            {
                continue;
            }
            self.send_with_media(peer, group, m);
        }
    }
    pub(crate) fn mark_selected_read(&mut self) {
        if !self.window_focused
            || self.settings
            || self.new_chat
            || self.new_group
            || self.conversation_info.is_some()
            || self.identity_panel.is_some()
            || !self.messages_at_bottom
        {
            return;
        }
        let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
        else {
            return;
        };
        for m in &mut c.messages {
            if !m.outgoing && !m.read {
                m.read = true;
                m.start_expiration(model::now_ms());
                self.dirty = true;
            }
        }
        c.recount_unread();
    }
    fn flush_read_receipts(&mut self, tx: &mut Tx) {
        if self.dirty
            || self.saved_generation != self.generation
            || self.demo
            || self.clock < self.receipt_retry_at
            || self.bridge.is_none()
        {
            return;
        }
        // The engine respects the account's phone-controlled privacy preference and also
        // emits the linked-device read sync. Never enable that preference from the desktop.
        let mut batches: HashMap<(String, String), Vec<(u64, u64)>> = HashMap::new();
        for c in &self.conversations {
            for m in &c.messages {
                if !m.outgoing
                    && m.read
                    && !m.read_receipt_sent
                    && !m.sender.is_empty()
                    && m.timestamp > 0
                {
                    batches
                        .entry((c.id.clone(), m.sender.clone()))
                        .or_default()
                        .push((m.id, m.latest_timestamp()));
                }
            }
        }
        for ((conversation, sender), messages) in batches {
            let pending: Vec<(u64, u64)> = self
                .pending
                .values()
                .filter_map(|p| {
                    if let Pending::Receipt(peer, ids) = p {
                        (peer == &conversation).then_some(ids)
                    } else {
                        None
                    }
                })
                .flatten()
                .copied()
                .collect();
            let messages: Vec<_> = messages
                .into_iter()
                .filter(|entry| !pending.contains(entry))
                .collect();
            for chunk in messages.chunks(100) {
                self.request("sendReceipt", json!({"recipient":sender,"targetTimestamp":chunk.iter().map(|(_, ts)| ts).collect::<Vec<_>>(),"type":"read"}), Pending::Receipt(conversation.clone(), chunk.to_vec()));
            }
        }
        if self.receipt_retry_at > self.clock {
            tx.wake_after(Duration::from_secs_f64(self.receipt_retry_at - self.clock));
        }
    }
    pub(crate) fn resolve_id(&self, id: &str) -> String {
        self.conversations
            .iter()
            .find(|c| c.matches(id))
            .map(|c| c.id.clone())
            .unwrap_or_else(|| id.to_owned())
    }
    pub(crate) fn receive(&mut self, v: &Value, tx: &mut Tx) {
        if let Some(account) = model::account(v)
            && !self.account_aliases.iter().any(|a| a == account)
            && self.account.as_deref() != Some(account)
        {
            return;
        }
        let Some(e) = model::envelope(v) else {
            return;
        };
        match e["syncMessage"]["type"].as_str() {
            Some("CONTACTS_SYNC") => {
                self.request("listContacts", json!({}), Pending::Contacts);
            }
            Some("GROUPS_SYNC") => {
                self.request("listGroups", json!({}), Pending::Groups);
            }
            _ => {}
        }
        if let Some(typing) = e.get("typingMessage") {
            self.receive_typing(e, typing, tx);
        }
        if let Some(receipt) = e.get("receiptMessage") {
            self.receive_receipt(e, receipt);
        }
        if let Some(reads) = e["syncMessage"]["readMessages"].as_array() {
            for read in reads {
                let sender = read["senderUuid"]
                    .as_str()
                    .or(read["senderNumber"].as_str())
                    .or(read["sender"].as_str());
                if let (Some(sender), Some(timestamp)) = (sender, read["timestamp"].as_u64()) {
                    let sender = self.resolve_id(sender);
                    let read_at = e["timestamp"]
                        .as_u64()
                        .filter(|at| *at > 0)
                        .unwrap_or_else(model::now_ms)
                        .min(model::now_ms());
                    self.sync_ledger
                        .read_at
                        .entry(sender.clone())
                        .or_default()
                        .entry(timestamp)
                        .and_modify(|at| *at = (*at).min(read_at))
                        .or_insert(read_at);
                    self.sync_ledger
                        .read_through
                        .entry(sender)
                        .and_modify(|ts| *ts = (*ts).max(timestamp))
                        .or_insert(timestamp);
                }
            }
            self.apply_sync_ledger();
            self.dirty = true;
        }
        self.receive_mutation(v);
        self.receive_expiration_update(v);
        let Some(m) = model::incoming(v) else {
            return;
        };
        let peer = self.resolve_id(&m.conversation);
        let index = self
            .conversations
            .iter()
            .position(|c| c.matches(&peer))
            .unwrap_or_else(|| {
                self.conversations
                    .push(Conversation::new(peer.clone(), m.name.clone(), m.group));
                self.conversations.len() - 1
            });
        let sender = if m.outgoing {
            self.account.clone().unwrap_or_default()
        } else {
            self.resolve_id(&m.sender)
        };
        let c = &mut self.conversations[index];
        for alias in m.aliases {
            if !c.aliases.contains(&alias) {
                c.aliases.push(alias);
            }
        }
        c.typing.remove(&sender);
        if let Some(existing) = c.messages.iter_mut().find(|x| {
            x.timestamp == m.timestamp
                && x.outgoing == m.outgoing
                && (m.outgoing || x.sender == sender)
        }) {
            existing.delivery.advance(Delivery::Sent);
            if m.outgoing {
                existing.start_expiration(
                    m.expiration_start
                        .unwrap_or(m.timestamp)
                        .min(model::now_ms()),
                );
            }
            self.dirty = true;
            return;
        }
        self.preferences.next_message_id += 1;
        c.messages.push(Message {
            id: self.preferences.next_message_id,
            body: m.body,
            sender,
            outgoing: m.outgoing,
            timestamp: m.timestamp,
            read: m.outgoing,
            delivery: Delivery::Sent,
            quote: m.quote,
            expires_in: m.expires_in,
            attachments: m.attachments,
            ..Message::default()
        });
        if m.outgoing {
            c.messages.last_mut().unwrap().start_expiration(
                m.expiration_start
                    .unwrap_or(m.timestamp)
                    .min(model::now_ms()),
            );
        }
        c.archived = false;
        c.active = true;
        if self.account.as_deref() == Some(&c.id) {
            c.name = "Note to Self".into();
        }
        c.sort_messages();
        c.recount_unread();
        self.apply_mutations();
        self.apply_sync_ledger();
        self.mark_selected_read();
        self.expire_messages(model::now_ms());
        self.dirty = true;
        if self.selected.as_ref() == Some(&peer) && self.messages_at_bottom {
            tx.scroll_to_bottom("messages");
        }
    }
    fn receive_receipt(&mut self, envelope: &Value, receipt: &Value) {
        let Some(sender) = model::source(envelope) else {
            return;
        };
        let sender = self.resolve_id(sender);
        let delivery = if receipt["isRead"] == true || receipt["isViewed"] == true {
            Delivery::Read
        } else if receipt["isDelivery"] == true {
            Delivery::Delivered
        } else {
            return;
        };
        if let Some(timestamps) = receipt["timestamps"].as_array() {
            let ledger = self.sync_ledger.receipts.entry(sender).or_default();
            for ts in timestamps.iter().filter_map(Value::as_u64) {
                ledger.entry(ts).or_default().advance(delivery);
            }
            self.apply_sync_ledger();
            self.dirty = true;
        }
    }
    pub(crate) fn apply_sync_ledger(&mut self) {
        for c in &mut self.conversations {
            for (sender, receipts) in &self.sync_ledger.receipts {
                if c.matches(sender) || (c.group && c.members.contains(sender)) {
                    for (timestamp, delivery) in receipts {
                        c.apply_receipt(sender, &[*timestamp], *delivery);
                    }
                }
            }
            for m in &mut c.messages {
                if m.outgoing
                    && m.expires_at.is_none()
                    && matches!(
                        m.delivery,
                        Delivery::Sent | Delivery::Delivered | Delivery::Read
                    )
                {
                    m.start_expiration(model::now_ms());
                }
                if !m.outgoing
                    && self
                        .sync_ledger
                        .read_through
                        .get(&m.sender)
                        .is_some_and(|ts| m.latest_timestamp() <= *ts)
                {
                    m.read = true;
                    m.read_receipt_sent = true;
                    let at = self
                        .sync_ledger
                        .read_at
                        .get(&m.sender)
                        .and_then(|reads| {
                            reads.range(m.latest_timestamp()..).map(|(_, at)| *at).min()
                        })
                        .unwrap_or_else(model::now_ms);
                    m.start_expiration(at);
                }
            }
            c.recount_unread();
        }
    }
    fn receive_typing(&mut self, envelope: &Value, typing: &Value, tx: &mut Tx) {
        if !self
            .engine_configuration
            .as_ref()
            .is_some_and(|c| c.typing_indicators)
        {
            return;
        }
        let Some(sender) = model::source(envelope) else {
            return;
        };
        if self.account_aliases.iter().any(|a| a == sender) {
            return;
        }
        let sender = self.resolve_id(sender);
        let peer = typing["groupId"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| sender.clone());
        let name = envelope["sourceName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&sender)
            .to_owned();
        let Some(c) = self.conversations.iter_mut().find(|c| c.matches(&peer)) else {
            return;
        };
        if c.blocked {
            return;
        }
        if typing["action"] == "STARTED" {
            c.typing.insert(
                sender,
                TypingPresence {
                    name,
                    until: self.clock + 15.,
                },
            );
            tx.wake_after(Duration::from_secs(15));
        } else if typing["action"] == "STOPPED" {
            c.typing.remove(&sender);
        }
    }
    fn typing_request(&mut self, peer: &str, stop: bool) {
        if !stop
            && !self
                .engine_configuration
                .as_ref()
                .is_some_and(|c| c.typing_indicators)
        {
            return;
        }
        let Some(c) = self.conversations.iter().find(|c| c.id == peer) else {
            return;
        };
        if c.blocked {
            return;
        }
        let mut params = json!({"stop":stop});
        params[if c.group { "groupId" } else { "recipient" }] = json!(peer);
        self.request("sendTyping", params, Pending::Transient);
    }
    pub(crate) fn stop_typing(&mut self) {
        if let Some(typing) = self.typing_outgoing.take() {
            self.typing_request(&typing.conversation, true);
        }
    }
    pub(crate) fn composer_changed(&mut self, tx: &mut Tx) {
        if self.demo {
            return;
        }
        self.draft_dirty = true;
        self.draft_save_at = Some(self.clock + 0.35);
        tx.wake_after(Duration::from_millis(350));
        let Some(peer) = self.selected.clone() else {
            return;
        };
        if self.composer.text.trim().is_empty()
            || !self.window_focused
            || self.conversation_info.is_some()
            || self.identity_panel.is_some()
            || !self.preferences.typing_indicators
            || self.new_group
            || !self
                .engine_configuration
                .as_ref()
                .is_some_and(|c| c.typing_indicators)
        {
            self.stop_typing();
            return;
        }
        if self
            .typing_outgoing
            .as_ref()
            .is_some_and(|t| t.conversation != peer)
        {
            self.stop_typing();
        }
        if let Some(typing) = &mut self.typing_outgoing {
            typing.last_edit = self.clock;
            if self.clock - typing.last_sent >= 10. {
                typing.last_sent = self.clock;
                self.typing_request(&peer, false);
            }
        } else {
            self.typing_request(&peer, false);
            self.typing_outgoing = Some(OutgoingTyping {
                conversation: peer,
                last_sent: self.clock,
                last_edit: self.clock,
            });
        }
        tx.wake_after(Duration::from_secs(3));
    }
    pub(crate) fn poll_timers(&mut self, tx: &mut Tx) -> bool {
        let mut changed = self.expire_messages(model::now_ms());
        if let Some(deadline) = self
            .conversations
            .iter()
            .flat_map(|c| &c.messages)
            .filter(|m| !m.hidden)
            .filter_map(|m| m.expires_at)
            .min()
        {
            tx.wake_after(Duration::from_millis(
                deadline.saturating_sub(model::now_ms()).clamp(1, 60_000),
            ));
        }
        if self.history_ready && self.bridge.is_some() && self.account.is_some() {
            if self.clock >= self.configuration_due
                && !self
                    .pending
                    .values()
                    .any(|p| matches!(p, Pending::Configuration))
            {
                self.request("getClientState", json!({}), Pending::Configuration);
                self.configuration_due = self.clock + 15.;
            }
            tx.wake_after(Duration::from_secs_f64(
                (self.configuration_due - self.clock).max(0.001),
            ));
        }
        if let Some(at) = self.reconnect_at {
            if self.clock >= at {
                self.connect();
                changed = true;
            }
            if let Some(at) = self.reconnect_at {
                tx.wake_after(Duration::from_secs_f64((at - self.clock).max(0.001)));
            }
        }
        if let Some(at) = self.draft_save_at {
            if self.clock >= at {
                self.dirty = true;
            } else {
                tx.wake_after(Duration::from_secs_f64(at - self.clock));
            }
        }
        for c in &mut self.conversations {
            let before = c.typing.len();
            c.typing.retain(|_, p| p.until > self.clock);
            changed |= before != c.typing.len();
            for p in c.typing.values() {
                tx.wake_after(Duration::from_secs_f64((p.until - self.clock).max(0.001)));
            }
        }
        if let Some(typing) = &self.typing_outgoing {
            let remaining = 3. - (self.clock - typing.last_edit);
            if remaining <= 0. {
                self.stop_typing();
            } else {
                tx.wake_after(Duration::from_secs_f64(remaining));
            }
        }
        if self.clock >= self.receipt_retry_at && self.saved_generation == self.generation {
            self.flush_read_receipts(tx);
        }
        self.mark_selected_read();
        self.flush_outbox();
        self.flush_actions();
        self.flush_pins();
        if !self.sync_ledger.pins.is_empty() && self.pin_retry_due > self.clock {
            tx.wake_after(Duration::from_secs_f64(self.pin_retry_due - self.clock));
        }
        self.poll_media();
        let wall = model::now_ms();
        if let Some(timestamp) = self
            .conversations
            .iter()
            .flat_map(|c| &c.messages)
            .filter(|m| m.outgoing && m.delivery == Delivery::Pending && m.timestamp >= wall)
            .map(|m| m.timestamp)
            .chain(
                self.sync_ledger
                    .actions
                    .iter()
                    .filter(|a| {
                        matches!(a.kind, model::ActionKind::Edit { .. })
                            && !a.failed
                            && a.id >= wall
                    })
                    .map(|a| a.id),
            )
            .min()
        {
            tx.wake_after(Duration::from_millis(
                timestamp.saturating_sub(wall).saturating_add(1).min(60_000),
            ));
        }
        changed
    }
    pub(crate) fn schedule_reconnect(&mut self) {
        if self.demo || (!self.history_ready && self.account.is_some()) {
            return;
        }
        let delay = 2u64.saturating_pow(self.reconnect_attempt.min(6)).min(60);
        self.reconnect_attempt += 1;
        self.reconnect_at = Some(self.clock + delay as f64);
    }
    pub(crate) fn archive_selected(&mut self) {
        self.stop_typing();
        self.remember_draft();
        if let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
        {
            c.draft = self.composer.text.clone();
            c.archived = !c.archived;
            self.dirty = true;
        }
        self.selected = None;
        self.persist();
    }
    pub(crate) fn merge_contacts(&mut self, items: &[Value], group: bool) {
        for item in items {
            let id = if group {
                item["id"].as_str()
            } else {
                item["uuid"]
                    .as_str()
                    .or(item["aci"].as_str())
                    .or(item["number"].as_str())
            };
            let Some(id) = id else {
                continue;
            };
            let aliases: Vec<String> = if group {
                vec![id.to_owned()]
            } else {
                ["uuid", "aci", "number"]
                    .iter()
                    .filter_map(|f| item[f].as_str().map(str::to_owned))
                    .collect()
            };
            let profile = [
                &item["profile"]["givenName"],
                &item["profile"]["familyName"],
            ]
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
            let name = item["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or((!profile.is_empty()).then_some(profile.as_str()))
                .unwrap_or(id)
                .to_owned();
            let matches: Vec<_> = self
                .conversations
                .iter()
                .enumerate()
                .filter(|(_, c)| c.group == group && aliases.iter().any(|a| c.matches(a)))
                .map(|(i, _)| i)
                .collect();
            let mut merged = Conversation::new(id.to_owned(), name, group);
            if matches.is_empty() {
                merged.archived = item["isArchived"].as_bool().unwrap_or(false);
            }
            for i in matches.into_iter().rev() {
                let c = self.conversations.remove(i);
                if self.selected.as_ref() == Some(&c.id) {
                    self.selected = Some(id.to_owned());
                }
                for p in self.pending.values_mut() {
                    match p {
                        Pending::Send(peer, _) | Pending::Receipt(peer, _) if peer == &c.id => {
                            *peer = id.to_owned()
                        }
                        _ => {}
                    }
                }
                merged.aliases.extend(c.aliases);
                merged.messages.extend(c.messages);
                merged.active |= c.active;
                merged.pinned = merged.pinned.max(c.pinned);
                merged.archived |= c.archived;
                merged.muted_until = merged.muted_until.max(c.muted_until);
                if !c.draft.is_empty() || c.draft_quote.is_some() || c.draft_edit.is_some() {
                    merged.draft = c.draft;
                    merged.draft_quote = c.draft_quote;
                    merged.draft_edit = c.draft_edit;
                    merged.draft_backup = c.draft_backup;
                }
                merged.draft_attachments.extend(c.draft_attachments);
                merged.expiration_updated_at =
                    merged.expiration_updated_at.max(c.expiration_updated_at);
                merged.typing.extend(c.typing);
            }
            merged.aliases.extend(aliases);
            merged.aliases.sort();
            merged.aliases.dedup();
            merged.blocked = item["isBlocked"].as_bool().unwrap_or(false);
            merged.expiration = item["messageExpirationTime"].as_u64().unwrap_or(0) as u32;
            if group {
                let people = |value: &Value| -> Vec<String> {
                    value
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|m| m.as_str().or(m["uuid"].as_str()).or(m["number"].as_str()))
                        .map(str::to_owned)
                        .collect()
                };
                merged.group_description =
                    item["description"].as_str().unwrap_or_default().to_owned();
                merged.group_admins = people(&item["admins"]);
                merged.group_pending = people(&item["pendingMembers"]);
                merged.group_requesting = people(&item["requestingMembers"]);
                merged.group_banned = people(&item["banned"]);
                merged.group_link = item["groupInviteLink"].as_str().map(str::to_owned);
                merged.group_member = item["isMember"].as_bool().unwrap_or(false);
                merged.group_terminated = item["isTerminated"].as_bool().unwrap_or(false);
                merged.permission_edit = item["permissionEditDetails"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                merged.permission_add = item["permissionAddMember"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                merged.permission_send = item["permissionSendMessage"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
            }
            merged.members = item["members"]
                .as_array()
                .map(|members| {
                    members
                        .iter()
                        .filter_map(|m| m.as_str().or(m["uuid"].as_str()).or(m["number"].as_str()))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            for m in &mut merged.messages {
                if !m.outgoing && merged.aliases.contains(&m.sender) && !group {
                    m.sender = id.to_owned();
                }
            }
            merged.sort_messages();
            merged.recount_unread();
            if self.account.as_deref() == Some(&merged.id) {
                merged.name = "Note to Self".into();
            }
            self.conversations.push(merged);
        }
        let aliases: HashMap<String, String> = self
            .conversations
            .iter()
            .flat_map(|c| c.aliases.iter().map(|a| (a.clone(), c.id.clone())))
            .collect();
        let mut message_ids = HashMap::new();
        for c in &mut self.conversations {
            for member in c
                .members
                .iter_mut()
                .chain(&mut c.group_admins)
                .chain(&mut c.group_pending)
                .chain(&mut c.group_requesting)
                .chain(&mut c.group_banned)
            {
                if let Some(id) = aliases.get(member) {
                    *member = id.clone();
                }
            }
            for m in &mut c.messages {
                if let Some(id) = aliases.get(&m.sender) {
                    m.sender = id.clone();
                }
                if let Some(quote) = &mut m.quote
                    && let Some(id) = aliases.get(&quote.author)
                {
                    quote.author = id.clone();
                }
            }
            message_ids.extend(c.reconcile_duplicates());
            if let Some(id) = c.draft_edit
                && let Some(new) = message_ids.get(&id)
            {
                c.draft_edit = Some(*new);
            }
        }
        if let Some(id) = self.editing
            && let Some(new) = message_ids.get(&id)
        {
            self.editing = Some(*new);
        }
        for pending in self.pending.values_mut() {
            match pending {
                Pending::Send(_, id) => {
                    if let Some(new) = message_ids.get(id) {
                        *id = *new;
                    }
                }
                Pending::Receipt(_, messages) => {
                    for (id, _) in messages {
                        if let Some(new) = message_ids.get(id) {
                            *id = *new;
                        }
                    }
                }
                Pending::Identities(peer)
                | Pending::VerifyIdentity(peer)
                | Pending::ConversationUpdate(peer, _)
                | Pending::GroupUpdate(peer) => {
                    if let Some(id) = aliases.get(peer) {
                        *peer = id.clone();
                    }
                }
                _ => {}
            }
        }
        if let Some(peer) = &mut self.conversation_info
            && let Some(id) = aliases.get(peer)
        {
            *peer = id.clone();
        }
        if let Some(panel) = &mut self.identity_panel
            && let Some(id) = aliases.get(&panel.peer)
        {
            panel.peer = id.clone();
        }
        for action in &mut self.sync_ledger.actions {
            if let Some(new) = message_ids.get(&action.message) {
                action.message = *new;
            }
            if let Some(id) = aliases.get(&action.conversation) {
                action.conversation = id.clone();
            }
            if let Some(id) = aliases.get(&action.author) {
                action.author = id.clone();
            }
        }
        for mutation in &mut self.sync_ledger.mutations {
            if let Some(id) = aliases.get(&mutation.conversation) {
                mutation.conversation = id.clone();
            }
            if let Some(id) = aliases.get(&mutation.author) {
                mutation.author = id.clone();
            }
            if let Some(id) = aliases.get(&mutation.actor) {
                mutation.actor = id.clone();
            }
        }
        for task in self.media_tasks.values_mut() {
            match task {
                crate::media::Task::Download(_, message, _)
                | crate::media::Task::Send(_, message) => {
                    if let Some(new) = message_ids.get(message) {
                        *message = *new;
                    }
                }
                _ => {}
            }
            let peer = match task {
                crate::media::Task::Draft(peer)
                | crate::media::Task::Download(peer, _, _)
                | crate::media::Task::Send(peer, _) => Some(peer),
                _ => None,
            };
            if let Some(peer) = peer
                && let Some(id) = aliases.get(peer)
            {
                *peer = id.clone();
            }
        }
        let mut receipts =
            std::collections::BTreeMap::<String, std::collections::BTreeMap<u64, Delivery>>::new();
        for (sender, messages) in std::mem::take(&mut self.sync_ledger.receipts) {
            let canonical = self.resolve_id(&sender);
            for (timestamp, delivery) in messages {
                receipts
                    .entry(canonical.clone())
                    .or_default()
                    .entry(timestamp)
                    .or_default()
                    .advance(delivery);
            }
        }
        self.sync_ledger.receipts = receipts;
        let mut reads = std::collections::BTreeMap::<String, u64>::new();
        for (sender, timestamp) in std::mem::take(&mut self.sync_ledger.read_through) {
            reads
                .entry(self.resolve_id(&sender))
                .and_modify(|t| *t = (*t).max(timestamp))
                .or_insert(timestamp);
        }
        self.sync_ledger.read_through = reads;
        let mut read_at =
            std::collections::BTreeMap::<String, std::collections::BTreeMap<u64, u64>>::new();
        for (sender, messages) in std::mem::take(&mut self.sync_ledger.read_at) {
            let sender = self.resolve_id(&sender);
            for (ts, at) in messages {
                read_at
                    .entry(sender.clone())
                    .or_default()
                    .entry(ts)
                    .and_modify(|old| *old = (*old).min(at))
                    .or_insert(at);
            }
        }
        self.sync_ledger.read_at = read_at;
        self.apply_mutations();
        self.apply_sync_ledger();
        self.mark_selected_read();
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::Bridge;
    use std::sync::mpsc::Receiver;

    #[allow(
        clippy::field_reassign_with_default,
        reason = "State's flushing Drop prevents struct update syntax"
    )]
    fn connected() -> (State, Receiver<Value>) {
        let (bridge, requests, _) = Bridge::mock();
        let mut state = State::default();
        state.account = Some("self".into());
        state.account_aliases = vec!["self".into()];
        state.bridge = Some(bridge);
        state.history_ready = true;
        state.configuration_due = 1000.;
        state.engine_configuration = Some(model::EngineConfiguration::default());
        state
            .conversations
            .push(Conversation::new("peer".into(), "Peer".into(), false));
        state.selected = Some("peer".into());
        (state, requests)
    }
    fn incoming(sender: &str, timestamp: u64, body: &str) -> Value {
        json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":sender,"sourceName":"Peer","dataMessage":{"message":body,"timestamp":timestamp}}}})
    }
    #[test]
    fn typing_start_refresh_stop_and_timeout_use_real_protocol_parameters() {
        let (mut s, requests) = connected();
        s.window_focused = true;
        s.composer.replace_text("hello");
        s.composer_changed(&mut Tx::default());
        let start = requests.try_recv().unwrap();
        assert_eq!(start["method"], "sendTyping");
        assert_eq!(
            start["params"],
            json!({"account":"self","recipient":"peer","stop":false})
        );
        s.clock = 1.;
        s.composer_changed(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.clock = 10.;
        s.composer_changed(&mut Tx::default());
        assert_eq!(requests.try_recv().unwrap()["params"]["stop"], false);
        s.clock = 12.9;
        s.poll_timers(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.clock = 13.;
        s.poll_timers(&mut Tx::default());
        assert_eq!(requests.try_recv().unwrap()["params"]["stop"], true);
        assert!(s.typing_outgoing.is_none());
    }
    #[test]
    fn incoming_typing_expires_and_stop_is_immediate() {
        let (mut s, _) = connected();
        let started = json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":"peer","sourceName":"Peer","typingMessage":{"action":"STARTED","timestamp":model::now_ms()}}}});
        s.receive(&started, &mut Tx::default());
        assert_eq!(s.conversations[0].typing.len(), 1);
        s.clock = 14.9;
        s.poll_timers(&mut Tx::default());
        assert_eq!(s.conversations[0].typing.len(), 1);
        s.clock = 15.;
        s.poll_timers(&mut Tx::default());
        assert!(s.conversations[0].typing.is_empty());
        s.receive(&started, &mut Tx::default());
        let mut stopped = started;
        stopped["params"]["envelope"]["typingMessage"]["action"] = json!("STOPPED");
        s.receive(&stopped, &mut Tx::default());
        assert!(s.conversations[0].typing.is_empty());
    }
    #[test]
    fn only_foreground_visible_conversations_emit_read_receipts() {
        let (mut s, requests) = connected();
        s.receive(&incoming("peer", 42, "hello"), &mut Tx::default());
        assert_eq!(s.conversations[0].unread, 1);
        s.flush_read_receipts(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.window_focused = true;
        s.messages_at_bottom = false;
        s.mark_selected_read();
        assert!(!s.conversations[0].messages[0].read);
        s.messages_at_bottom = true;
        s.mark_selected_read();
        assert!(s.conversations[0].messages[0].read);
        s.dirty = false; // The storage integration test below verifies this commit boundary.
        s.flush_read_receipts(&mut Tx::default());
        let receipt = requests.try_recv().unwrap();
        assert_eq!(receipt["method"], "sendReceipt");
        assert_eq!(
            receipt["params"],
            json!({"account":"self","recipient":"peer","targetTimestamp":[42],"type":"read"})
        );
        s.flush_read_receipts(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.event(
            json!({"id":receipt["id"],"result":{"timestamp":99,"results":[{"type":"SUCCESS"}]}}),
            &mut Tx::default(),
        );
        assert!(s.conversations[0].messages[0].read_receipt_sent);
    }
    #[test]
    fn read_receipt_before_send_ack_is_never_downgraded() {
        let (mut s, _) = connected();
        s.conversations[0].messages.push(Message {
            id: 42,
            timestamp: 42,
            outgoing: true,
            delivery: Delivery::Pending,
            ..Message::default()
        });
        s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","receiptMessage":{"isRead":true,"timestamps":[42]}}}}), &mut Tx::default());
        assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Read);
        s.pending.insert(100, Pending::Send("peer".into(), 42));
        s.event(
            json!({"id":100,"result":{"timestamp":42,"results":[{"type":"SUCCESS"}]}}),
            &mut Tx::default(),
        );
        s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","receiptMessage":{"isDelivery":true,"timestamps":[42]}}}}), &mut Tx::default());
        assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Read);
        assert_eq!(
            s.conversations[0].messages[0].receipts["peer"],
            Delivery::Read
        );
    }
    #[test]
    fn phone_read_sync_reconciles_late_messages_without_resending_receipts() {
        let (mut s, requests) = connected();
        s.receive(&json!({"method":"receive","params":{"envelope":{"syncMessage":{"readMessages":[{"senderUuid":"peer","timestamp":50}]}}}}), &mut Tx::default());
        s.receive(&incoming("peer", 42, "older"), &mut Tx::default());
        s.receive(&incoming("peer", 60, "newer"), &mut Tx::default());
        assert!(s.conversations[0].messages[0].read);
        assert!(s.conversations[0].messages[0].read_receipt_sent);
        assert!(!s.conversations[0].messages[1].read);
        assert_eq!(s.conversations[0].unread, 1);
        s.flush_read_receipts(&mut Tx::default());
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn early_receipts_reconcile_when_phone_sent_sync_arrives() {
        let (mut s, _) = connected();
        s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","receiptMessage":{"isDelivery":true,"timestamps":[42]}}}}), &mut Tx::default());
        let sent = json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":"self","syncMessage":{"sentMessage":{"destinationUuid":"peer","message":"from phone","timestamp":42}}}}});
        s.receive(&sent, &mut Tx::default());
        s.receive(&sent, &mut Tx::default());
        assert_eq!(s.conversations[0].messages.len(), 1);
        assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Delivered);
    }
    #[test]
    fn contact_uuid_reconciliation_preserves_draft_pin_and_selected_chat() {
        let (mut s, _) = connected();
        s.conversations.clear();
        let mut old = Conversation::new("+12025550123".into(), "Old".into(), false);
        old.pinned = 2;
        old.draft = "keep this".into();
        old.messages.push(Message {
            id: 42,
            timestamp: 42,
            body: "hello".into(),
            sender: old.id.clone(),
            ..Message::default()
        });
        s.selected = Some(old.id.clone());
        s.composer.replace_text("keep this");
        s.conversations.push(old);
        s.merge_contacts(
            &[json!({"uuid":"peer-aci","number":"+12025550123","name":"Peer"})],
            false,
        );
        assert_eq!(s.conversations.len(), 1);
        assert_eq!(s.conversations[0].id, "peer-aci");
        assert_eq!(s.conversations[0].pinned, 2);
        assert_eq!(s.conversations[0].draft, "keep this");
        assert_eq!(s.conversations[0].messages[0].sender, "peer-aci");
        assert_eq!(s.selected.as_deref(), Some("peer-aci"));
    }
    #[test]
    fn other_account_events_and_non_member_group_receipts_are_isolated() {
        let (mut s, _) = connected();
        let mut other = incoming("peer", 42, "wrong account");
        other["params"]["account"] = json!("other");
        s.receive(&other, &mut Tx::default());
        assert!(s.conversations[0].messages.is_empty());
        let mut group = Conversation::new("group".into(), "Group".into(), true);
        group.members = vec!["member".into()];
        group.messages.push(Message {
            id: 50,
            timestamp: 50,
            outgoing: true,
            delivery: Delivery::Sent,
            ..Message::default()
        });
        s.conversations.push(group);
        s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"outsider","receiptMessage":{"isRead":true,"timestamps":[50]}}}}), &mut Tx::default());
        assert_eq!(s.conversations[1].messages[0].delivery, Delivery::Sent);
    }
    #[test]
    fn alias_merge_keeps_deletion_tombstones_and_cancels_queued_edits() {
        let (mut s, _) = connected();
        s.conversations.clear();
        let mut phone = Conversation::new("+12025550123".into(), "Phone".into(), false);
        phone.messages.push(Message {
            id: 1,
            timestamp: 100,
            sender: phone.id.clone(),
            body: "private content".into(),
            ..Default::default()
        });
        let mut uuid = Conversation::new("peer-aci".into(), "Peer".into(), false);
        uuid.messages.push(Message {
            id: 2,
            timestamp: 100,
            sender: uuid.id.clone(),
            hidden: true,
            tombstone_timestamps: vec![200],
            ..Default::default()
        });
        uuid.draft_edit = Some(2);
        uuid.draft = "private edit".into();
        s.sync_ledger.actions.push(model::QueuedAction {
            id: 300,
            conversation: uuid.id.clone(),
            message: 2,
            target: 100,
            author: uuid.id.clone(),
            kind: model::ActionKind::Edit {
                body: "private edit".into(),
                quote: None,
            },
            failed: false,
        });
        s.conversations = vec![phone, uuid];
        s.merge_contacts(
            &[json!({"uuid":"peer-aci","number":"+12025550123","name":"Peer"})],
            false,
        );
        let c = &s.conversations[0];
        assert_eq!(c.messages.len(), 1);
        assert!(c.messages[0].hidden);
        assert!(c.messages[0].body.is_empty());
        assert!(c.messages[0].matches_timestamp(200));
        assert!(c.draft.is_empty() && c.draft_edit.is_none());
        assert!(s.sync_ledger.actions.is_empty());
        s.receive(
            &incoming("peer-aci", 100, "private content"),
            &mut Tx::default(),
        );
        assert!(s.conversations[0].messages[0].hidden);
        assert!(s.conversations[0].messages[0].body.is_empty());
    }
    fn pump(s: &mut State, condition: impl Fn(&State) -> bool) {
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while !condition(s) {
            s.poll_history(&mut Tx::default());
            s.poll_timers(&mut Tx::default());
            if std::time::Instant::now() > until {
                panic!("Storage did not reach the requested state: {:?}", s.error);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn durable_outbox_commits_before_send_and_restarts_with_the_same_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, requests) = connected();
        s.history_ready = false;
        s.store = Some(storage::Worker::test(dir.path(), "self"));
        pump(&mut s, |s| {
            s.history_ready && s.generation > 0 && s.saved_generation == s.generation
        });
        for _ in requests.try_iter() {}
        s.choose("peer".into());
        s.composer.replace_text("survive restart");
        s.send(&mut Tx::default());
        assert!(
            requests.try_recv().is_err(),
            "Network send must wait for the commit acknowledgement"
        );
        let generation = s.generation;
        pump(&mut s, |s| {
            s.saved_generation >= generation
                && s.pending.values().any(|p| matches!(p, Pending::Send(..)))
        });
        let send = requests.try_iter().find(|r| r["method"] == "send").unwrap();
        let timestamp = send["params"]["timestamp"].as_u64().unwrap();
        assert!(timestamp > 0);
        drop(s); // No server acknowledgement: the durable row must remain in the outbox.
        let (mut restored, requests) = connected();
        restored.history_ready = false;
        restored.store = Some(storage::Worker::test(dir.path(), "self"));
        pump(&mut restored, |s| {
            s.history_ready && s.generation > 0 && s.saved_generation == s.generation
        });
        let retry = requests.try_iter().find(|r| r["method"] == "send").unwrap();
        assert_eq!(retry["params"]["timestamp"], timestamp);
        assert_eq!(retry["params"]["message"], "survive restart");
        assert_eq!(
            restored
                .conversations
                .iter()
                .find(|c| c.id == "peer")
                .unwrap()
                .messages
                .len(),
            1
        );
    }
    #[test]
    fn read_receipts_wait_for_read_state_to_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (mut s, requests) = connected();
        s.history_ready = false;
        s.store = Some(storage::Worker::test(dir.path(), "self"));
        pump(&mut s, |s| {
            s.history_ready && s.generation > 0 && s.saved_generation == s.generation
        });
        for _ in requests.try_iter() {}
        s.receive(
            &incoming("peer", 42, "read only after commit"),
            &mut Tx::default(),
        );
        s.window_focused = true;
        s.choose("peer".into());
        s.mark_selected_read();
        s.flush_read_receipts(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.persist();
        s.flush_read_receipts(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        let generation = s.generation;
        pump(&mut s, |s| s.saved_generation == generation);
        let receipt = requests
            .try_iter()
            .find(|r| r["method"] == "sendReceipt")
            .unwrap();
        assert_eq!(receipt["params"]["targetTimestamp"], json!([42]));
    }
    #[test]
    fn unknown_or_disabled_phone_typing_setting_never_sends_start() {
        let (mut s, requests) = connected();
        s.window_focused = true;
        s.engine_configuration = None;
        s.composer.replace_text("private draft");
        s.composer_changed(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        s.engine_configuration = Some(model::EngineConfiguration {
            typing_indicators: false,
            ..model::EngineConfiguration::default()
        });
        s.composer_changed(&mut Tx::default());
        assert!(requests.try_recv().is_err());
        let incoming = json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","typingMessage":{"action":"STARTED"}}}});
        s.receive(&incoming, &mut Tx::default());
        assert!(s.conversations[0].typing.is_empty());
    }
    #[test]
    fn phone_disabling_typing_clears_active_indicators_and_sends_stop() {
        let (mut s, requests) = connected();
        s.window_focused = true;
        s.composer.replace_text("hello");
        s.composer_changed(&mut Tx::default());
        assert_eq!(requests.try_recv().unwrap()["params"]["stop"], false);
        s.pending.insert(100, Pending::Configuration);
        s.event(json!({"id":100,"result":{"version":1,"readReceipts":false,"typingIndicators":false,"linkPreviews":false,"unidentifiedDeliveryIndicators":false}}), &mut Tx::default());
        assert_eq!(requests.try_recv().unwrap()["params"]["stop"], true);
        assert!(s.typing_outgoing.is_none());
        assert!(!s.engine_configuration.as_ref().unwrap().typing_indicators);
    }
}
