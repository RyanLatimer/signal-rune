//! Account-scoped, authenticated-encrypted history. SQLite only ever sees ciphertext.
//! A single worker owns the connection and commits before acknowledging a generation.
pub mod media;
use crate::model::{Conversation, Message, Preferences, SyncLedger};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use fs2::FileExt;
use hmac::{Hmac, Mac};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub conversations: Vec<Conversation>,
    pub preferences: Preferences,
    pub selected: Option<String>,
    pub sync: SyncLedger,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum Record {
    Conversation {
        value: Box<Conversation>,
    },
    Message {
        conversation: String,
        value: Box<Message>,
    },
    Preferences {
        value: Preferences,
        selected: Option<String>,
    },
    Sync {
        value: SyncLedger,
    },
}
pub enum Event {
    Ready(Snapshot, String),
    Saved(u64),
    Error(String),
    Media(u64, Result<media::ResultData, String>),
}
enum Command {
    Save(u64, Snapshot),
    Stop,
    Media(u64, media::Operation),
}
pub struct Worker {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
    join: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn start(account: String, wake: rune::Waker) -> Result<Self, String> {
        let root = directories::ProjectDirs::from("org", "signal-rune", "Signal Rune")
            .ok_or("Cannot locate history directory")?
            .data_local_dir()
            .join("history");
        Ok(Self::spawn(move || Store::open(&root, &account), wake))
    }
    #[cfg(test)]
    pub(crate) fn test(root: &Path, account: &str) -> Self {
        let root = root.to_path_buf();
        let account = account.to_owned();
        Self::spawn(
            move || {
                private_dir(&root)?;
                let lock = Store::lock(&root)?;
                Store::initialize(&root, &account, &[6; 32], "Test key".into(), lock)
            },
            std::sync::Arc::new(|| {}),
        )
    }
    fn spawn(
        open: impl FnOnce() -> Result<Store, String> + Send + 'static,
        wake: rune::Waker,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (events, event_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            let result = open();
            let mut store = match result {
                Ok(store) => store,
                Err(e) => {
                    let _ = events.send(Event::Error(e));
                    wake();
                    return;
                }
            };
            match store.load() {
                Ok(snapshot) => {
                    let _ = events.send(Event::Ready(snapshot, store.protection.clone()));
                    wake();
                }
                Err(e) => {
                    let _ = events.send(Event::Error(e));
                    wake();
                    return;
                }
            }
            for command in rx {
                match command {
                    Command::Save(generation, snapshot) => {
                        match store.save(&snapshot) {
                            Ok(()) => {
                                let _ = events.send(Event::Saved(generation));
                            }
                            Err(e) => {
                                let _ = events.send(Event::Error(e));
                            }
                        }
                        wake();
                    }
                    Command::Stop => break,
                    Command::Media(ticket, operation) => {
                        let result = store.media(operation);
                        let _ = events.send(Event::Media(ticket, result));
                        wake();
                    }
                }
            }
        });
        Self {
            commands: tx,
            events: event_rx,
            join: Some(join),
        }
    }
    pub fn save(&self, generation: u64, snapshot: Snapshot) -> Result<(), String> {
        self.commands
            .send(Command::Save(generation, snapshot))
            .map_err(|_| "History worker stopped".into())
    }
    pub fn media(&self, ticket: u64, operation: media::Operation) -> Result<(), String> {
        self.commands
            .send(Command::Media(ticket, operation))
            .map_err(|_| "History worker stopped".into())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
fn digest(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn private_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("Cannot create history directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn key_for(dir: &Path, account: &str) -> Result<(Zeroizing<Vec<u8>>, String), String> {
    let fallback = dir.join("key");
    let entry = keyring::Entry::new("org.signal-rune.history", &digest(account))
        .map_err(|_| "Cannot initialize system credential storage")?;
    let existing = entry.get_secret();
    if let Ok(key) = existing.as_ref() {
        if key.len() != 32 {
            return Err("Invalid history encryption key in system credential storage".into());
        }
        return Ok((Zeroizing::new(key.clone()), "System keychain".into()));
    }
    if fallback.exists() {
        let mut key = Zeroizing::new(Vec::new());
        File::open(&fallback)
            .and_then(|mut f| f.read_to_end(&mut key))
            .map_err(|_| "Cannot read protected history key")?;
        if key.len() != 32 {
            return Err("Invalid protected history key".into());
        }
        if entry.set_secret(&key).is_ok() {
            std::fs::remove_file(fallback).map_err(
                |_| "History key moved to keychain but local key file could not be removed",
            )?;
            return Ok((key, "System keychain".into()));
        }
        return Ok((key, "Private key file (system keychain unavailable)".into()));
    }
    if dir.join("history.db").exists() {
        return Err("History encryption key is unavailable. Unlock your system keychain and retry; existing history has been preserved.".into());
    }
    let key = Zeroizing::new(Aes256Gcm::generate_key(&mut OsRng).to_vec());
    if entry.set_secret(&key).is_ok() {
        return Ok((key, "System keychain".into()));
    }
    #[cfg(target_os = "linux")]
    {
        // Like Signal's basic-text fallback, this protects backups of the DB but not a user
        // who can also read the key file. Report the actual protection in Settings.
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        let mut file = options
            .open(fallback)
            .map_err(|_| "Cannot create protected history key")?;
        file.write_all(&key)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot persist protected history key")?;
        Ok((key, "Private key file (system keychain unavailable)".into()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("Cannot store history key. Unlock your system keychain and retry.".into())
    }
}
struct Store {
    db: Connection,
    cipher: Aes256Gcm,
    index_key: Zeroizing<Vec<u8>>,
    account: String,
    hashes: HashMap<String, String>,
    protection: String,
    _lock: File,
    pending_media: HashSet<String>,
}
impl Store {
    fn open(root: &Path, account: &str) -> Result<Self, String> {
        let dir = root.join(digest(account));
        private_dir(&dir)?;
        let lock = Self::lock(&dir)?;
        let (key, protection) = key_for(&dir, account)?;
        Self::initialize(&dir, account, &key, protection, lock)
    }
    fn lock(dir: &Path) -> Result<File, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "This account's history is already open in another Signal Rune window")?;
        Ok(lock)
    }
    fn initialize(
        dir: &Path,
        account: &str,
        key: &[u8],
        protection: String,
        lock: File,
    ) -> Result<Self, String> {
        let db = Connection::open(dir.join("history.db")).map_err(|e| e.to_string())?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 3 {
            return Err("This history was created by a newer Signal Rune version; it has not been modified.".into());
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON; CREATE TABLE IF NOT EXISTS records (id TEXT PRIMARY KEY NOT NULL, value BLOB NOT NULL); CREATE TABLE IF NOT EXISTS media (id TEXT PRIMARY KEY NOT NULL, value BLOB NOT NULL); PRAGMA user_version=3;").map_err(|e| e.to_string())?;
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid history key length")?;
        let mut index =
            <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| "Invalid history index key")?;
        index.update(b"signal-rune-history-index-v1\0");
        index.update(account.as_bytes());
        let index_key = Zeroizing::new(index.finalize().into_bytes().to_vec());
        Ok(Self {
            db,
            cipher,
            index_key,
            account: account.to_owned(),
            hashes: HashMap::new(),
            protection,
            _lock: lock,
            pending_media: HashSet::new(),
        })
    }
    fn aad(&self, id: &str) -> Vec<u8> {
        format!("signal-rune-history-v1\0{}\0{id}", self.account).into_bytes()
    }
    fn record_id(&self, identity: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.index_key)
            .expect("HMAC accepts an arbitrary key length");
        mac.update(identity.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
    fn seal(&self, id: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut sealed = nonce.to_vec();
        sealed.extend(
            self.cipher
                .encrypt(
                    &nonce,
                    Payload {
                        msg: plaintext,
                        aad: &self.aad(id),
                    },
                )
                .map_err(|_| "History encryption failed")?,
        );
        Ok(sealed)
    }
    fn unseal(&self, id: &str, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        if ciphertext.len() < 28 {
            return Err("History record is truncated; it has not been overwritten.".into());
        }
        self.cipher.decrypt(Nonce::from_slice(&ciphertext[..12]), Payload { msg: &ciphertext[12..], aad: &self.aad(id) }).map_err(|_| "History authentication failed. Restore the correct key or a valid backup; existing history has been preserved.".into())
    }
    fn load(&mut self) -> Result<Snapshot, String> {
        let mut snapshot = Snapshot::default();
        let mut messages = Vec::new();
        let mut stmt = self
            .db
            .prepare("SELECT id, value FROM records")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, value) = row.map_err(|e| e.to_string())?;
            let plaintext = Zeroizing::new(self.unseal(&id, &value)?);
            self.hashes.insert(id, digest(&plaintext));
            match serde_json::from_slice::<Record>(&plaintext)
                .map_err(|_| "History record could not be decoded; it has not been overwritten")?
            {
                Record::Conversation { value } => snapshot.conversations.push(*value),
                Record::Message {
                    conversation,
                    value,
                } => messages.push((conversation, *value)),
                Record::Preferences { value, selected } => {
                    snapshot.preferences = value;
                    snapshot.selected = selected;
                }
                Record::Sync { value } => snapshot.sync = value,
            }
        }
        for (id, mut message) in messages {
            // An interrupted send is left in the outbox with its original timestamp. Resending
            // that exact timestamp lets the other Signal client deduplicate it.
            if message.outgoing {
                message.read = true;
            }
            if let Some(c) = snapshot.conversations.iter_mut().find(|c| c.id == id) {
                c.messages.push(message);
            }
        }
        for c in &mut snapshot.conversations {
            c.sort_messages();
            c.recount_unread();
        }
        Ok(snapshot)
    }
    fn save(&mut self, snapshot: &Snapshot) -> Result<(), String> {
        let mut desired = HashSet::new();
        let mut updates = Vec::new();
        let mut add = |identity: String, record: Record| -> Result<(), String> {
            let id = self.record_id(&identity);
            desired.insert(id.clone());
            let plaintext = Zeroizing::new(
                serde_json::to_vec(&record).map_err(|_| "Cannot encode history record")?,
            );
            let hash = digest(&plaintext);
            if self.hashes.get(&id) != Some(&hash) {
                updates.push((id.clone(), self.seal(&id, &plaintext)?, hash));
            }
            Ok(())
        };
        add(
            "preferences".into(),
            Record::Preferences {
                value: snapshot.preferences.clone(),
                selected: snapshot.selected.clone(),
            },
        )?;
        add(
            "sync".into(),
            Record::Sync {
                value: snapshot.sync.clone(),
            },
        )?;
        for c in &snapshot.conversations {
            let mut metadata = c.clone();
            metadata.messages.clear();
            metadata.typing.clear();
            add(
                format!("conversation\0{}", c.id),
                Record::Conversation {
                    value: Box::new(metadata),
                },
            )?;
            for m in &c.messages {
                add(
                    format!("message\0{}\0{}\0{}", c.id, m.id, m.sender),
                    Record::Message {
                        conversation: c.id.clone(),
                        value: Box::new(m.clone()),
                    },
                )?;
            }
        }
        let remove: Vec<_> = self
            .hashes
            .keys()
            .filter(|k| !desired.contains(*k))
            .cloned()
            .collect();
        let media_refs = self.media_references(snapshot);
        let media_remove = self.unreferenced_media(&media_refs)?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        for (id, value, _) in &updates {
            tx.execute("INSERT INTO records(id,value) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET value=excluded.value", params![id, value]).map_err(|e| e.to_string())?;
        }
        for id in &remove {
            tx.execute("DELETE FROM records WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        for id in media_remove {
            tx.execute("DELETE FROM media WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        tx.commit()
            .map_err(|e| format!("History commit failed: {e}"))?;
        for (id, _, hash) in updates {
            self.hashes.insert(id, hash);
        }
        for id in remove {
            self.hashes.remove(&id);
        }
        self.pending_media.retain(|id| !media_refs.contains(id));
        Ok(())
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        // Explicitly release the account lock instead of relying on the final descriptor
        // close. Shared or inherited Unix descriptors must not keep a closed store locked.
        let _ = FileExt::unlock(&self._lock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(path: &Path, account: &str, key: &[u8]) -> Store {
        private_dir(path).unwrap();
        let lock = Store::lock(path).unwrap();
        Store::initialize(path, account, key, "Test key".into(), lock).unwrap()
    }
    #[test]
    fn encrypted_restart_restores_messages_drafts_pins_and_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let key = [7u8; 32];
        let mut c = Conversation::new("private-recipient".into(), "Private Name".into(), false);
        c.pinned = 4;
        c.draft = "private draft".into();
        c.messages.push(Message {
            id: 42,
            timestamp: 42,
            body: "private message text".into(),
            ..Message::default()
        });
        {
            let mut store = open(dir.path(), "account-a", &key);
            store
                .save(&Snapshot {
                    conversations: vec![c],
                    ..Snapshot::default()
                })
                .unwrap();
        }
        let raw = std::fs::read(dir.path().join("history.db")).unwrap();
        for secret in [
            "private-recipient",
            "Private Name",
            "private draft",
            "private message text",
        ] {
            assert!(!raw.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
        let restored = open(dir.path(), "account-a", &key).load().unwrap();
        assert_eq!(restored.conversations[0].draft, "private draft");
        assert_eq!(restored.conversations[0].pinned, 4);
        assert_eq!(
            restored.conversations[0].messages[0].body,
            "private message text"
        );
    }
    #[test]
    fn wrong_key_or_account_cannot_read_history() {
        let dir = tempfile::tempdir().unwrap();
        {
            open(dir.path(), "account-a", &[1; 32])
                .save(&Snapshot::default())
                .unwrap();
        }
        assert!(open(dir.path(), "account-a", &[2; 32]).load().is_err());
        assert!(open(dir.path(), "account-b", &[1; 32]).load().is_err());
    }
    #[test]
    fn record_indexes_are_keyed_and_legacy_rows_migrate_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path(), "a", &[3; 32]);
        let identity = "conversation\0+12025550123";
        assert_ne!(store.record_id(identity), digest(identity));
        let other = tempfile::tempdir().unwrap();
        assert_ne!(
            store.record_id(identity),
            open(other.path(), "a", &[4; 32]).record_id(identity)
        );
        let other = tempfile::tempdir().unwrap();
        assert_ne!(
            store.record_id(identity),
            open(other.path(), "b", &[3; 32]).record_id(identity)
        );
        let legacy_id = digest(identity);
        let record = Record::Conversation {
            value: Box::new(Conversation::new(
                "+12025550123".into(),
                "Peer".into(),
                false,
            )),
        };
        let sealed = store
            .seal(&legacy_id, &serde_json::to_vec(&record).unwrap())
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO records VALUES(?1,?2)",
                params![legacy_id, sealed],
            )
            .unwrap();
        store.db.pragma_update(None, "user_version", 1).unwrap();
        drop(store);
        let mut store = open(dir.path(), "a", &[3; 32]);
        let snapshot = store.load().unwrap();
        assert_eq!(snapshot.conversations[0].name, "Peer");
        store.save(&snapshot).unwrap();
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT count(*) FROM records WHERE id=?1",
                    [&legacy_id],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(store.load().unwrap().conversations[0].id, "+12025550123");
    }
    #[test]
    fn newer_schema_is_not_modified() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path(), "a", &[3; 32]);
        store.db.pragma_update(None, "user_version", 4).unwrap();
        drop(store);
        assert!(
            Store::initialize(
                dir.path(),
                "a",
                &[3; 32],
                "Test".into(),
                Store::lock(dir.path()).unwrap()
            )
            .is_err()
        );
        let db = Connection::open(dir.path().join("history.db")).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            4
        );
    }
    #[test]
    fn tampered_records_are_rejected_and_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open(dir.path(), "a", &[3; 32]);
        store.save(&Snapshot::default()).unwrap();
        store
            .db
            .execute("UPDATE records SET value = zeroblob(64)", [])
            .unwrap();
        assert!(store.load().is_err());
        assert_eq!(
            store
                .db
                .query_row("SELECT length(value) FROM records", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            64
        );
    }
    #[test]
    fn expired_or_deleted_records_are_removed_and_concurrent_open_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = open(dir.path(), "a", &[3; 32]);
        let mut c = Conversation::new("peer".into(), "Peer".into(), false);
        c.messages.push(Message {
            id: 1,
            body: "temporary".into(),
            ..Message::default()
        });
        store
            .save(&Snapshot {
                conversations: vec![c.clone()],
                ..Snapshot::default()
            })
            .unwrap();
        c.messages.clear();
        store
            .save(&Snapshot {
                conversations: vec![c],
                ..Snapshot::default()
            })
            .unwrap();
        assert!(store.load().unwrap().conversations[0].messages.is_empty());
        assert!(Store::lock(dir.path()).is_err());
    }
    #[test]
    fn worker_shutdown_releases_the_account_lock_before_restart() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..12 {
            let worker = Worker::test(dir.path(), "self");
            match worker
                .events
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
            {
                Event::Ready(..) => {}
                Event::Error(e) => panic!("History failed to reopen: {e}"),
                _ => panic!("Expected loaded history"),
            }
            drop(worker);
        }
    }
    #[cfg(unix)]
    #[test]
    fn retained_descriptor_does_not_keep_a_closed_store_locked() {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path(), "self", &[3; 32]);
        let duplicate = store._lock.try_clone().unwrap();
        drop(store);
        let reopened = Store::lock(dir.path())
            .expect("Store teardown must release the lock even when a descriptor copy is retained");
        drop(reopened);
        drop(duplicate);
    }
}
