//! Durable pin intents are separate from remote acknowledgements and ordinary message sends.
use crate::{
    Pending, State,
    model::{self, Conversation, PinIntent, PinState},
};
use serde_json::{Value, json};
impl State {
    pub(crate) fn toggle_pin(&mut self, peer: &str) {
        let Some(c) = self.conversations.iter().find(|c| c.id == peer) else {
            return;
        };
        let pinned = c.pinned == 0;
        let group = c.group;
        let unknown = self
            .pin_state
            .as_ref()
            .map_or(0, |state| state.count.saturating_sub(state.pins.len()));
        if pinned && self.conversations.iter().filter(|c| c.pinned > 0).count() + unknown >= 4 {
            self.error = Some("You can pin up to four conversations. Unpin one first.".into());
            return;
        }
        let next = self
            .conversations
            .iter()
            .map(|c| c.pinned)
            .max()
            .unwrap_or(0)
            + 1;
        if let Some(c) = self.conversations.iter_mut().find(|c| c.id == peer) {
            c.pinned = if pinned { next } else { 0 };
            if pinned {
                c.archived = false;
            }
            c.active = true;
        }
        if !self.demo {
            self.pin_retry_due = self.clock;
            let id =
                model::now_ms().max(self.preferences.last_outgoing_timestamp.saturating_add(1));
            self.preferences.last_outgoing_timestamp = id;
            self.sync_ledger.pins.push(PinIntent {
                id,
                conversation: peer.into(),
                group,
                pinned,
            });
        }
        self.dirty = true;
        self.persist();
    }
    pub(crate) fn flush_pins(&mut self) {
        if self.demo
            || !self.history_ready
            || self.dirty
            || self.generation != self.saved_generation
            || self.bridge.is_none()
            || self.clock < self.pin_retry_due
            || !self.pin_state.as_ref().is_some_and(|s| s.supported)
        {
            return;
        }
        for pin in self.sync_ledger.pins.clone() {
            if self.pin_accepted.contains(&pin.id)
                || self.pending.values().any(|p| matches!(p, Pending::Pin(_)))
            {
                continue;
            }
            let mut params = json!({"operationId":pin.id,"pinned":pin.pinned});
            params[if pin.group { "groupId" } else { "recipient" }] = json!(pin.conversation);
            self.request("setConversationPin", params, Pending::Pin(pin.id));
            return;
        }
        if !self.sync_ledger.pins.is_empty()
            && !self.pending.values().any(|p| matches!(p, Pending::PinSync))
        {
            self.request("syncConversationPins", json!({}), Pending::PinSync);
            self.pin_retry_due = self.clock + 15.;
        }
    }
    pub(crate) fn pin_result(&mut self, id: u64, result: &Value) {
        if result["operationId"].as_u64() != Some(id) {
            self.pin_retry_due = self.clock + 30.;
            self.pin_sync_error = Some(
                "The engine did not confirm the pin operation. The local change has been kept."
                    .into(),
            );
            return;
        }
        if result["synced"] == true {
            self.sync_ledger.pins.retain(|p| p.id != id);
            self.pin_accepted.remove(&id);
            self.dirty = true;
        } else {
            self.pin_accepted.insert(id);
        }
        self.pin_sync_error = None;
        // Send remaining durable intents before scheduling a network retry. Their engine
        // writes are ordered and acknowledged independently of any background storage job.
        self.flush_pins();
    }
    pub(crate) fn receive_pin_state(&mut self, value: &Value) {
        let Ok(state) = serde_json::from_value::<PinState>(value.clone()) else {
            return;
        };
        if !state.supported {
            self.pin_state = Some(state);
            return;
        }
        let before = self.sync_ledger.pins.len();
        self.sync_ledger
            .pins
            .retain(|p| !state.completed.contains(&p.id));
        self.pin_accepted
            .retain(|id| self.sync_ledger.pins.iter().any(|p| &p.id == id));
        self.dirty |= before != self.sync_ledger.pins.len();
        let mut pins = Vec::new();
        for pin in &state.pins {
            let id = self.resolve_id(&pin.id);
            if !self.conversations.iter().any(|c| c.id == id) {
                self.conversations.push(Conversation::new(
                    id.clone(),
                    if self.account.as_deref() == Some(&id) {
                        "Note to Self".into()
                    } else {
                        id.clone()
                    },
                    pin.group,
                ));
            }
            if !pins.contains(&id) {
                pins.push(id);
            }
        }
        if !state.ready {
            self.pin_state = Some(state);
            return;
        }
        if !self.preferences.pin_sync_initialized {
            let mut local: Vec<_> = self.conversations.iter().collect();
            local.sort_by_key(|c| c.pinned);
            for c in local {
                if c.pinned > 0
                    && !pins.iter().any(|id| c.matches(id))
                    && !self
                        .sync_ledger
                        .pins
                        .iter()
                        .any(|p| c.matches(&p.conversation))
                {
                    let id = model::now_ms()
                        .max(self.preferences.last_outgoing_timestamp.saturating_add(1));
                    self.preferences.last_outgoing_timestamp = id;
                    self.sync_ledger.pins.push(PinIntent {
                        id,
                        conversation: c.id.clone(),
                        group: c.group,
                        pinned: true,
                    });
                }
            }
            self.preferences.pin_sync_initialized = true;
            self.pin_retry_due = self.clock;
            self.dirty = true;
        }
        for c in &mut self.conversations {
            if self
                .sync_ledger
                .pins
                .iter()
                .any(|intent| c.matches(&intent.conversation))
            {
                continue;
            }
            let rank = pins
                .iter()
                .position(|id| c.matches(id))
                .map_or(0, |i| i as u64 + 1);
            if c.pinned != rank {
                c.pinned = rank;
                self.dirty = true;
            }
            if rank > 0 {
                c.active = true;
            }
        }
        if state.pending == 0 && self.sync_ledger.pins.is_empty() {
            self.pin_sync_error = None;
        }
        self.pin_state = Some(state);
    }
    pub(crate) fn pin_sync_status(&self) -> String {
        if let Some(error) = &self.pin_sync_error {
            return error.clone();
        }
        if !self.sync_ledger.pins.is_empty() {
            return format!(
                "{} pin change(s) waiting to sync",
                self.sync_ledger.pins.len()
            );
        }
        match &self.pin_state {
            Some(state) if state.supported && state.ready && state.pending == 0 => {
                "Conversation pins are synchronized with your linked devices.".into()
            }
            Some(state) if state.supported => "Conversation pins are synchronizing…".into(),
            _ => "Pin synchronization is waiting for the bundled engine’s account storage.".into(),
        }
    }
}
#[cfg(test)]
mod tests;
