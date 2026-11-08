//! File selection, encrypted media jobs, and protocol attachments; disk work stays off the UI.
use crate::{
    Pending, State,
    model::{Attachment, Delivery, Message},
    storage::media::{Operation, ResultData, Source},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rune::Tx;
use serde_json::json;
use std::{collections::HashSet, path::PathBuf};

pub enum Task {
    Draft(String),
    Download(String, u64, String),
    Preview(String),
    Send(String, u64),
    Export,
}

pub fn safe_name(name: &str) -> String {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("Attachment")
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        })
        .take(200)
        .collect();
    if name.is_empty() || matches!(name.as_str(), "." | "..") {
        "Attachment".into()
    } else {
        name
    }
}

pub fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1048576.)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
fn data_uri(a: &Attachment, bytes: &[u8]) -> String {
    let mime = if a.content_type.contains('/')
        && a.content_type
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/+-._".contains(&c))
    {
        &a.content_type
    } else {
        "application/octet-stream"
    };
    let filename: String = safe_name(&a.name)
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!(
        "data:{mime};filename={filename};base64,{}",
        STANDARD.encode(bytes)
    )
}
impl State {
    fn media_job(&mut self, task: Task, op: Operation) {
        let Some(store) = &self.store else { return };
        self.serial += 1;
        let ticket = self.serial;
        match store.media(ticket, op) {
            Ok(()) => {
                self.media_tasks.insert(ticket, task);
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(crate) fn choose_attachment(&mut self, tx: &mut Tx) {
        if self.demo || !self.history_ready || self.editing.is_some() {
            return;
        }
        let Some(peer) = self.selected.clone() else {
            return;
        };
        tx.open_file(
            rune::FileDialogOptions {
                title: "Attach a file".into(),
                ..Default::default()
            },
            move |s: &mut State, result, _| match result {
                Ok(Some(path)) => s.import_files(&peer, &[path]),
                Ok(None) => {}
                Err(_) => s.error = Some("Could not open the file picker".into()),
            },
        );
    }
    pub(crate) fn import_files(&mut self, peer: &str, paths: &[PathBuf]) {
        if self.demo || !self.history_ready || self.editing.is_some() {
            return;
        }
        let count = self
            .conversations
            .iter()
            .find(|c| c.id == peer)
            .map_or(32, |c| c.draft_attachments.len())
            + self
                .media_tasks
                .values()
                .filter(|t| matches!(t,Task::Draft(p) if p==peer))
                .count();
        if count + paths.len() > 32 {
            self.error = Some("Attach up to 32 files per message".into());
            return;
        }
        for path in paths {
            self.media_job(
                Task::Draft(peer.into()),
                Operation::Import(Source::File(path.clone()), Attachment::default()),
            );
        }
    }
    pub(crate) fn remove_draft_attachment(&mut self, index: usize) {
        if let Some(c) = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter_mut().find(|c| &c.id == id))
            && index < c.draft_attachments.len()
        {
            c.draft_attachments.remove(index);
            self.dirty = true;
            self.persist();
        }
    }
    pub(crate) fn save_attachment(&mut self, a: &Attachment, tx: &mut Tx) {
        let Some(id) = a.media_id.clone() else {
            self.media_failures.remove(&a.id);
            self.poll_media();
            return;
        };
        tx.save_file(
            rune::FileDialogOptions {
                title: "Save attachment".into(),
                file_name: Some(safe_name(&a.name)),
                ..Default::default()
            },
            move |s: &mut State, result, _| match result {
                Ok(Some(path)) => s.media_job(Task::Export, Operation::Export(id, path)),
                Ok(None) => {}
                Err(_) => s.error = Some("Could not open the Save dialog".into()),
            },
        );
    }
    pub(crate) fn poll_media(&mut self) {
        if self.demo
            || !self.history_ready
            || self.dirty
            || self.generation != self.saved_generation
        {
            return;
        }
        let downloads: Vec<_> = self
            .conversations
            .iter()
            .flat_map(|c| {
                c.messages
                    .iter()
                    .filter(|m| !m.hidden && !m.deleted)
                    .flat_map(|m| {
                        m.attachments
                            .iter()
                            .filter(|a| a.media_id.is_none() && !a.id.is_empty())
                            .map(|a| (c.id.clone(), m.id, a.clone()))
                    })
            })
            .collect();
        for (peer, message, a) in downloads {
            if self.media_failures.contains_key(&a.id)
                || self
                    .media_tasks
                    .values()
                    .any(|t| matches!(t,Task::Download(_,_,id) if id==&a.id))
            {
                continue;
            }
            self.media_job(
                Task::Download(peer, message, a.id.clone()),
                Operation::Import(Source::Engine(a.id.clone()), a),
            );
        }
        let visible: HashSet<_> = self
            .selected
            .as_ref()
            .and_then(|id| self.conversations.iter().find(|c| &c.id == id))
            .into_iter()
            .flat_map(|c| {
                c.draft_attachments.iter().chain(
                    c.messages
                        .iter()
                        .filter(|m| !m.hidden && !m.deleted)
                        .flat_map(|m| &m.attachments),
                )
            })
            .filter(|a| matches!(a.content_type.as_str(), "image/png" | "image/jpeg"))
            .filter_map(|a| a.media_id.clone())
            .collect();
        self.media_images.retain(|id, _| visible.contains(id));
        if self.media_cache.is_some() {
            for id in visible {
                if !self.media_images.contains_key(&id)
                    && !self.media_failures.contains_key(&id)
                    && !self
                        .media_tasks
                        .values()
                        .any(|t| matches!(t,Task::Preview(p) if p==&id))
                {
                    self.media_job(Task::Preview(id.clone()), Operation::Read(vec![id]));
                }
            }
        }
    }
    pub(crate) fn send_with_media(&mut self, peer: String, group: bool, m: Message) {
        if m.attachments.is_empty() {
            self.send_payload(peer, group, &m, vec![]);
            return;
        }
        if self
            .media_tasks
            .values()
            .any(|t| matches!(t,Task::Send(_,id) if *id==m.id))
        {
            return;
        }
        let Some(ids) = m
            .attachments
            .iter()
            .map(|a| a.media_id.clone())
            .collect::<Option<Vec<_>>>()
        else {
            self.delivery(&peer, m.id, Delivery::Failed, 0);
            self.error = Some("Attachment must be available before sending".into());
            return;
        };
        self.media_job(Task::Send(peer, m.id), Operation::Read(ids));
    }
    fn send_payload(&mut self, peer: String, group: bool, m: &Message, attachments: Vec<String>) {
        let mut params = json!({"message":m.body,"timestamp":m.timestamp});
        if self.account.as_deref() == Some(&peer) {
            params["noteToSelf"] = json!(true);
        } else {
            params[if group { "groupId" } else { "recipient" }] = json!(peer);
        }
        if let Some(quote) = &m.quote {
            params["quoteTimestamp"] = json!(quote.timestamp);
            params["quoteAuthor"] = json!(quote.author);
            params["quoteMessage"] = json!(quote.body);
        }
        if !attachments.is_empty() {
            params["attachment"] = json!(attachments);
        }
        self.request("send", params, Pending::Send(peer, m.id));
    }
    pub(crate) fn media_result(&mut self, ticket: u64, result: Result<ResultData, String>) {
        let Some(task) = self.media_tasks.remove(&ticket) else {
            return;
        };
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                match &task {
                    Task::Download(_, _, id) | Task::Preview(id) => {
                        self.media_failures.insert(id.clone(), e.clone());
                    }
                    Task::Send(peer, id) => self.delivery(peer, *id, Delivery::Failed, 0),
                    _ => {}
                }
                self.error = Some(e);
                return;
            }
        };
        match (task, result) {
            (Task::Draft(peer), ResultData::Imported(a)) => {
                if let Some(c) = self.conversations.iter_mut().find(|c| c.id == peer) {
                    c.draft_attachments.push(a);
                    self.dirty = true;
                }
            }
            (Task::Download(peer, message, remote), ResultData::Imported(a)) => {
                self.dirty = true;
                if let Some(m) = self
                    .conversations
                    .iter_mut()
                    .find(|c| c.id == peer)
                    .and_then(|c| {
                        c.messages
                            .iter_mut()
                            .find(|m| m.id == message && !m.hidden && !m.deleted)
                    })
                    && let Some(old) = m.attachments.iter_mut().find(|a| a.id == remote)
                {
                    *old = a;
                    self.dirty = true;
                }
            }
            (Task::Preview(id), ResultData::Bytes(mut data)) => {
                if let (Some(cache), Some(bytes)) = (&self.media_cache, data.pop()) {
                    self.media_images
                        .insert(id, cache.load(std::sync::Arc::<[u8]>::from(bytes.to_vec())));
                }
            }
            (Task::Send(peer, id), ResultData::Bytes(data)) => {
                if self.dirty || self.generation != self.saved_generation || self.bridge.is_none() {
                    return;
                }
                if let Some((group, m)) = self
                    .conversations
                    .iter()
                    .find(|c| c.id == peer && !c.blocked)
                    .and_then(|c| {
                        c.messages
                            .iter()
                            .find(|m| {
                                m.id == id
                                    && !m.hidden
                                    && !m.deleted
                                    && m.delivery == Delivery::Pending
                            })
                            .map(|m| (c.group, m.clone()))
                    })
                    && m.attachments.len() == data.len()
                {
                    let attachments = m
                        .attachments
                        .iter()
                        .zip(data)
                        .map(|(a, b)| data_uri(a, &b))
                        .collect();
                    self.send_payload(peer, group, &m, attachments);
                }
            }
            _ => {}
        }
        self.persist();
    }
}

#[cfg(test)]
mod tests;
