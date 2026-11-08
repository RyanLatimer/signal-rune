//! Durable message actions and authenticated sender-scoped remote mutations.
use crate::{
    Pending, State,
    model::{
        self, ActionKind, Delivery, DraftBackup, EditVersion, Message, Mutation, QueuedAction,
        Quote,
    },
};
use rune::PointerPress;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct MessageContext {
    pub conversation: String,
    pub message: u64,
    pub point: PointerPress,
}
impl State {
    pub(crate) fn remember_draft(&mut self) {
        if let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
        {
            c.draft = self.composer.text.clone();
            c.draft_quote = self.reply_to.clone();
            c.draft_edit = self.editing;
            c.draft_backup = self.edit_backup.clone();
        }
    }
    pub(crate) fn display_name(&self, id: &str) -> String {
        if self.account.as_deref() == Some(id) || self.account_aliases.iter().any(|a| a == id) {
            return "You".into();
        }
        self.conversations
            .iter()
            .find(|c| c.matches(id))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| {
                if id.is_empty() {
                    "Unknown".into()
                } else {
                    id.to_owned()
                }
            })
    }
    pub(crate) fn edit_last_message(&mut self) {
        let Some(peer) = self.selected.clone() else {
            return;
        };
        let note = self.account.as_ref().is_some_and(|a| {
            self.conversations
                .iter()
                .any(|c| c.id == peer && c.matches(a))
        });
        if let Some(id) = self
            .conversations
            .iter()
            .find(|c| c.id == peer)
            .and_then(|c| c.messages.iter().rev().find(|m| m.can_edit(note)))
            .map(|m| m.id)
        {
            self.start_edit(&peer, id);
        }
    }
    pub(crate) fn find_message(&self, peer: &str, id: u64) -> Option<&Message> {
        self.conversations
            .iter()
            .find(|c| c.matches(peer))?
            .messages
            .iter()
            .find(|m| m.id == id && !m.hidden)
    }
    fn canonical_author(&self, id: &str) -> String {
        if self.account_aliases.iter().any(|a| a == id) {
            self.account.clone().unwrap_or_else(|| id.to_owned())
        } else {
            self.resolve_id(id)
        }
    }
    pub(crate) fn start_reply(&mut self, peer: &str, id: u64) {
        self.message_context = None;
        let Some(m) = self.find_message(peer, id) else {
            return;
        };
        if m.deleted || m.hidden {
            return;
        }
        let author = if m.outgoing {
            self.account.clone().unwrap_or_default()
        } else {
            m.sender.clone()
        };
        let quote = Quote {
            timestamp: m.latest_timestamp(),
            author,
            body: m.body.clone(),
        };
        if self.selected.as_deref() != Some(peer) {
            self.choose(peer.to_owned());
        }
        if self.editing.is_some() {
            self.cancel_edit();
        }
        self.reply_to = Some(quote);
        self.composer.focused = true;
        self.dirty = true;
        self.persist();
    }
    pub(crate) fn start_edit(&mut self, peer: &str, id: u64) {
        self.message_context = None;
        let note_to_self = self.account.as_ref().is_some_and(|a| {
            self.conversations
                .iter()
                .any(|c| c.matches(peer) && c.matches(a))
        });
        let Some(m) = self.find_message(peer, id) else {
            return;
        };
        if !m.can_edit(note_to_self) {
            self.error = Some("This message can no longer be edited.".into());
            return;
        }
        let body = m.body.clone();
        let quote = m.quote.clone();
        if self.selected.as_deref() != Some(peer) {
            self.choose(peer.to_owned());
        }
        if self.editing.is_none() {
            self.edit_backup = Some(DraftBackup {
                body: self.composer.text.clone(),
                quote: self.reply_to.clone(),
            });
        }
        self.editing = Some(id);
        self.reply_to = quote;
        self.composer.replace_text(body);
        self.composer.focused = true;
        self.dirty = true;
        self.persist();
    }
    pub(crate) fn cancel_edit(&mut self) {
        self.editing = None;
        if let Some(backup) = self.edit_backup.take() {
            self.composer.replace_text(backup.body);
            self.reply_to = backup.quote;
        } else {
            self.composer.clear();
            self.reply_to = None;
        }
        self.dirty = true;
        self.persist();
    }
    pub(crate) fn submit_edit(&mut self) {
        let (Some(peer), Some(id)) = (self.selected.clone(), self.editing) else {
            return;
        };
        let body = self.composer.text.trim().to_owned();
        if body.is_empty() {
            return;
        }
        if self.queue_action(
            &peer,
            id,
            ActionKind::Edit {
                body,
                quote: self.reply_to.clone(),
            },
        ) {
            self.cancel_edit();
        }
    }
    pub(crate) fn queue_action(&mut self, peer: &str, id: u64, kind: ActionKind) -> bool {
        self.message_context = None;
        self.reaction_target = None;
        if self.demo {
            return false;
        }
        if !self.history_ready {
            self.error = Some("History must be available before changing a message.".into());
            return false;
        }
        let Some(c) = self.conversations.iter().find(|c| c.matches(peer)) else {
            return false;
        };
        if c.blocked {
            self.error = Some("Unblock this conversation first.".into());
            return false;
        }
        let Some(m) = c.messages.iter().find(|m| m.id == id && !m.hidden) else {
            return false;
        };
        if m.deleted || m.hidden {
            return false;
        }
        let note_to_self = self.account.as_ref().is_some_and(|a| c.matches(a));
        if matches!(kind, ActionKind::Edit { .. }) && !m.can_edit(note_to_self) {
            self.error = Some("This message can no longer be edited.".into());
            return false;
        }
        if matches!(kind, ActionKind::Delete) && !m.can_delete_for_everyone() {
            self.error = Some(
                "Only your messages sent in the last 24 hours can be deleted for everyone.".into(),
            );
            return false;
        }
        let action = QueuedAction {
            id: model::now_ms().max(self.preferences.last_outgoing_timestamp.saturating_add(1)),
            conversation: c.id.clone(),
            message: id,
            target: m.latest_timestamp(),
            author: if m.outgoing {
                self.account.clone().unwrap_or_default()
            } else {
                m.sender.clone()
            },
            kind,
            failed: false,
        };
        self.preferences.last_outgoing_timestamp = action.id;
        self.sync_ledger.actions.push(action);
        self.dirty = true;
        self.persist();
        true
    }
    pub(crate) fn flush_actions(&mut self) {
        if self.dirty
            || !self.history_ready
            || self.saved_generation != self.generation
            || self.demo
            || self.bridge.is_none()
        {
            return;
        }
        for action in self.sync_ledger.actions.clone() {
            if action.failed
                || self
                    .pending
                    .values()
                    .any(|p| matches!(p, Pending::Action(id) if *id == action.id))
            {
                continue;
            }
            let Some(c) = self
                .conversations
                .iter()
                .find(|c| c.matches(&action.conversation))
            else {
                continue;
            };
            if c.blocked {
                continue;
            }
            let mut params = json!({});
            if self.account.as_ref().is_some_and(|a| c.matches(a)) {
                params["noteToSelf"] = json!(true);
            } else {
                params[if c.group { "groupId" } else { "recipient" }] = json!(c.id);
            }
            let method = match &action.kind {
                ActionKind::Reaction { emoji, remove } => {
                    params["emoji"] = json!(emoji);
                    params["remove"] = json!(remove);
                    params["targetAuthor"] = json!(action.author);
                    params["targetTimestamp"] = json!(action.target);
                    "sendReaction"
                }
                ActionKind::Edit { body, quote } => {
                    params["message"] = json!(body);
                    params["editTimestamp"] = json!(action.target);
                    if let Some(q) = quote {
                        params["quoteTimestamp"] = json!(q.timestamp);
                        params["quoteAuthor"] = json!(q.author);
                        params["quoteMessage"] = json!(q.body);
                    }
                    "send"
                }
                ActionKind::Delete => {
                    params["targetTimestamp"] = json!(action.target);
                    "remoteDelete"
                }
            };
            self.request(method, params, Pending::Action(action.id));
        }
    }
    pub(crate) fn action_finished(&mut self, id: u64, result: &Value) {
        let Some(index) = self.sync_ledger.actions.iter().position(|a| a.id == id) else {
            return;
        };
        let failed = result["timestamp"].as_u64().is_none()
            || result["results"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|r| r["type"].as_str().is_some_and(|t| t != "SUCCESS"))
            });
        if failed {
            self.sync_ledger.actions[index].failed = true;
            self.error = Some(
                "The message action could not be completed. Open message details to retry.".into(),
            );
            self.dirty = true;
            return;
        }
        let action = self.sync_ledger.actions.remove(index);
        let actor = self.account.clone().unwrap_or_default();
        self.record_mutation(Mutation {
            conversation: action.conversation,
            author: action.author,
            actor,
            target: action.target,
            at: result["timestamp"].as_u64().unwrap_or(0),
            kind: action.kind,
        });
        self.apply_mutations();
        self.apply_sync_ledger();
        self.mark_selected_read();
        self.dirty = true;
    }
    pub(crate) fn retry_message(&mut self, peer: &str, id: u64) {
        if let Some(m) = self
            .conversations
            .iter_mut()
            .find(|c| c.matches(peer))
            .and_then(|c| c.messages.iter_mut().find(|m| m.id == id))
            && m.delivery == Delivery::Failed
        {
            m.delivery = Delivery::Pending;
            self.dirty = true;
        }
        for action in &mut self.sync_ledger.actions {
            if action.conversation == peer && action.message == id && action.failed {
                action.failed = false;
                self.dirty = true;
            }
        }
        self.message_context = None;
        self.error = None;
        self.persist();
    }
    pub(crate) fn delete_for_me(&mut self, peer: &str, id: u64) {
        if self.selected.as_deref() == Some(peer) && self.editing == Some(id) {
            self.cancel_edit();
        }
        if let Some(c) = self.conversations.iter_mut().find(|c| c.matches(peer)) {
            if let Some(m) = c.messages.iter_mut().find(|m| m.id == id) {
                m.tombstone_timestamps
                    .extend(m.edit_history.iter().map(|v| v.timestamp));
                m.hidden = true;
                m.body.clear();
                m.attachments.clear();
                m.quote = None;
                m.reactions.clear();
                m.edit_history.clear();
                m.read = true;
                m.read_receipt_sent = true;
            }
            c.recount_unread();
            self.dirty = true;
        }
        self.sync_ledger
            .actions
            .retain(|a| !(a.conversation == peer && a.message == id));
        self.message_context = None;
        self.message_details = None;
        self.delete_confirmation = None;
        self.purge_tombstone_mutations();
        self.persist();
    }
    pub(crate) fn receive_mutation(&mut self, value: &Value) {
        let Some(context) = model::data_context(value) else {
            return;
        };
        let data = context.data;
        let peer = self.resolve_id(&context.conversation);
        let actor = if context.outgoing {
            self.account.clone().unwrap_or_default()
        } else {
            self.canonical_author(&context.sender)
        };
        let at = data["timestamp"]
            .as_u64()
            .or_else(|| model::envelope(value).and_then(|e| e["timestamp"].as_u64()))
            .unwrap_or(0);
        if at == 0 || actor.is_empty() {
            return;
        }
        let mutation = if let Some(target) = context.edit_target {
            data["message"].as_str().map(|body| Mutation {
                conversation: peer,
                author: actor.clone(),
                actor,
                target,
                at,
                kind: ActionKind::Edit {
                    body: body.to_owned(),
                    quote: model::quote(&data["quote"]),
                },
            })
        } else if let Some(reaction) = data.get("reaction") {
            let author = reaction["targetAuthorUuid"]
                .as_str()
                .or(reaction["targetAuthorNumber"].as_str())
                .or(reaction["targetAuthor"].as_str());
            let target = reaction["targetSentTimestamp"].as_u64();
            let emoji = reaction["emoji"].as_str().unwrap_or_default();
            if let (Some(author), Some(target)) = (author, target) {
                (emoji.len() <= 128 && !emoji.contains(['\n', '\r'])).then(|| Mutation {
                    conversation: peer,
                    author: self.canonical_author(author),
                    actor,
                    target,
                    at,
                    kind: ActionKind::Reaction {
                        emoji: emoji.to_owned(),
                        remove: reaction["isRemove"] == true,
                    },
                })
            } else {
                None
            }
        } else {
            data["remoteDelete"]["timestamp"]
                .as_u64()
                .or(data["remoteDelete"]["targetSentTimestamp"].as_u64())
                .map(|target| Mutation {
                    conversation: peer,
                    author: actor.clone(),
                    actor,
                    target,
                    at,
                    kind: ActionKind::Delete,
                })
        };
        if let Some(mutation) = mutation {
            self.record_mutation(mutation);
            self.apply_mutations();
            self.apply_sync_ledger();
            self.mark_selected_read();
            self.dirty = true;
        }
    }
    fn record_mutation(&mut self, mutation: Mutation) {
        let duplicate = self.sync_ledger.mutations.iter_mut().find(|m| {
            m.conversation == mutation.conversation
                && m.author == mutation.author
                && m.actor == mutation.actor
                && m.target == mutation.target
                && match (&m.kind, &mutation.kind) {
                    (
                        ActionKind::Reaction {
                            emoji: old,
                            remove: old_remove,
                        },
                        ActionKind::Reaction { emoji, remove },
                    ) => old_remove == remove && (!remove || old == emoji),
                    (ActionKind::Delete, ActionKind::Delete) => true,
                    (ActionKind::Edit { .. }, ActionKind::Edit { .. }) => m.at == mutation.at,
                    _ => false,
                }
        });
        if let Some(old) = duplicate {
            if mutation.at >= old.at {
                *old = mutation;
            }
        } else {
            self.sync_ledger.mutations.push(mutation);
        }
    }
    pub(crate) fn apply_mutations(&mut self) {
        // Reconcile aliases when a contact's ACI arrives after its phone-number events.
        let normalized: Vec<_> = self
            .sync_ledger
            .mutations
            .iter()
            .cloned()
            .map(|mut m| {
                m.conversation = self.resolve_id(&m.conversation);
                m.author = self.canonical_author(&m.author);
                m.actor = self.canonical_author(&m.actor);
                m
            })
            .collect();
        self.sync_ledger.mutations = normalized;
        let mut mutations = self.sync_ledger.mutations.clone();
        mutations.sort_by_key(|m| m.at);
        let own = self.account.as_deref().unwrap_or_default();
        for c in &mut self.conversations {
            let note_to_self = c.matches(own);
            let applicable: Vec<_> = mutations
                .iter()
                .filter(|m| c.matches(&m.conversation))
                .collect();
            for mutation in applicable {
                for m in &mut c.messages {
                    let author = if m.outgoing && m.sender.is_empty() {
                        own
                    } else {
                        &m.sender
                    };
                    if m.hidden
                        || !m.matches_timestamp(mutation.target)
                        || author != mutation.author
                    {
                        continue;
                    }
                    match &mutation.kind {
                        ActionKind::Reaction { emoji, remove } if !m.deleted => {
                            if *remove {
                                if m.reactions.get(&mutation.actor) == Some(emoji) {
                                    m.reactions.remove(&mutation.actor);
                                }
                            } else {
                                m.reactions.insert(mutation.actor.clone(), emoji.clone());
                            }
                        }
                        ActionKind::Edit { body, quote }
                            if mutation.actor == mutation.author
                                && !m.deleted
                                && mutation.at > m.latest_timestamp()
                                && m.edit_history.len() < 10
                                && (note_to_self
                                    || mutation.at.saturating_sub(m.timestamp) < 86_400_000) =>
                        {
                            m.edit_history.push(EditVersion {
                                timestamp: m.latest_timestamp(),
                                body: m.body.clone(),
                                quote: m.quote.clone(),
                                delivery: m.delivery,
                                receipts: m.receipts.clone(),
                            });
                            m.body = body.clone();
                            if quote.is_none() {
                                m.quote = None;
                            }
                            m.edited = true;
                            m.edited_at = Some(mutation.at);
                            m.delivery = Delivery::Sent;
                            m.receipts.clear();
                            m.read = m.outgoing;
                            m.read_receipt_sent = m.outgoing;
                        }
                        ActionKind::Delete
                            if mutation.actor == mutation.author
                                && mutation.at.saturating_sub(m.latest_timestamp())
                                    < 86_400_000 =>
                        {
                            m.tombstone_timestamps
                                .extend(m.edit_history.iter().map(|v| v.timestamp));
                            m.deleted = true;
                            m.body.clear();
                            m.attachments.clear();
                            m.quote = None;
                            m.reactions.clear();
                            m.edit_history.clear();
                            m.read = true;
                            m.read_receipt_sent = true;
                        }
                        _ => {}
                    }
                }
            }
            c.recount_unread();
        }
        self.purge_tombstone_mutations();
    }
    pub(crate) fn purge_tombstone_mutations(&mut self) {
        self.sync_ledger.mutations.retain(|mutation| {
            if matches!(mutation.kind, ActionKind::Delete) {
                return true;
            }
            !self
                .conversations
                .iter()
                .filter(|c| c.matches(&mutation.conversation))
                .flat_map(|c| &c.messages)
                .any(|m| {
                    (m.deleted || m.hidden)
                        && (m.sender == mutation.author
                            || (m.outgoing && self.account.as_deref() == Some(&mutation.author)))
                        && m.matches_timestamp(mutation.target)
                })
        });
    }
}

#[cfg(test)]
mod tests;
