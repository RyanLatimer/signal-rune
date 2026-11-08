//! Encrypted attachment blobs share the history worker and its transaction boundary.
use super::*;
use crate::model::Attachment;
use std::path::PathBuf;

pub const MAX_BYTES: u64 = 100 * 1024 * 1024;
pub enum Source {
    File(PathBuf),
    Engine(String),
}
pub enum Operation {
    Import(Source, Attachment),
    Read(Vec<String>),
    Export(String, PathBuf),
}
pub enum ResultData {
    Imported(Attachment),
    Bytes(Vec<Zeroizing<Vec<u8>>>),
    Exported,
}
pub fn engine_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.len() > 255
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err("Invalid attachment identifier".into());
    }
    Ok(root.join("attachments").join(id))
}
impl Store {
    fn engine_media_id(&self, id: &str) -> String {
        self.record_id(&format!("engine-media\0{id}"))
    }
    pub(super) fn media_references(&self, snapshot: &Snapshot) -> HashSet<String> {
        snapshot
            .conversations
            .iter()
            .flat_map(|c| {
                c.draft_attachments
                    .iter()
                    .chain(c.messages.iter().flat_map(|m| &m.attachments))
            })
            .filter_map(|a| {
                a.media_id
                    .clone()
                    .or_else(|| (!a.id.is_empty()).then(|| self.engine_media_id(&a.id)))
            })
            .collect()
    }
    pub(super) fn unreferenced_media(&self, refs: &HashSet<String>) -> Result<Vec<String>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM media")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut remove = Vec::new();
        for id in ids {
            let id = id.map_err(|e| e.to_string())?;
            if !refs.contains(&id) && !self.pending_media.contains(&id) {
                remove.push(id);
            }
        }
        Ok(remove)
    }
    fn read_media(&self, id: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let sealed: Vec<u8> = self
            .db
            .query_row("SELECT value FROM media WHERE id=?1", [id], |r| r.get(0))
            .map_err(|_| "Attachment is unavailable")?;
        Ok(Zeroizing::new(
            self.unseal(&format!("media:{id}"), &sealed)?,
        ))
    }
    pub(super) fn media(&mut self, op: Operation) -> Result<ResultData, String> {
        match op {
            Operation::Import(source, mut attachment) => {
                let (path, id, engine) = match source {
                    Source::File(path) => {
                        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
                        (
                            path,
                            self.record_id(&format!("picked-media\0{nonce:?}")),
                            false,
                        )
                    }
                    Source::Engine(remote) => {
                        let path = engine_path(&crate::transport::data_dir()?, &remote)?;
                        (path, self.engine_media_id(&remote), true)
                    }
                };
                let exists: bool = self
                    .db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM media WHERE id=?1)",
                        [&id],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if !exists {
                    let metadata = std::fs::symlink_metadata(&path).map_err(
                        |_| "Attachment file is unavailable. Retry after downloading it.",
                    )?;
                    if !metadata.is_file() || metadata.len() > MAX_BYTES {
                        return Err("Choose a regular file up to 100 MB".into());
                    }
                    let mut bytes = Zeroizing::new(Vec::new());
                    File::open(&path)
                        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
                        .map_err(|_| "Cannot read attachment")?;
                    if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
                        return Err("Choose a nonempty file up to 100 MB".into());
                    }
                    attachment.size = bytes.len() as u64;
                    if !engine {
                        attachment.name = crate::media::safe_name(
                            path.file_name()
                                .and_then(|p| p.to_str())
                                .unwrap_or("Attachment"),
                        );
                        attachment.content_type = mime_guess::from_path(&path)
                            .first_or_octet_stream()
                            .to_string();
                    }
                    let sealed = self.seal(&format!("media:{id}"), &bytes)?;
                    self.db
                        .execute("INSERT INTO media VALUES(?1,?2)", params![id, sealed])
                        .map_err(|e| e.to_string())?;
                }
                // Remote imports are scheduled only after their message metadata commits.
                // Their deterministic keyed ID makes a crash before the UI ack recoverable.
                if engine && path.exists() {
                    std::fs::remove_file(&path).map_err(|_|"Encrypted attachment saved, but engine staging file could not be removed")?;
                }
                if !engine {
                    self.pending_media.insert(id.clone());
                }
                attachment.media_id = Some(id);
                Ok(ResultData::Imported(attachment))
            }
            Operation::Read(ids) => {
                let mut bytes = Vec::new();
                for id in ids {
                    bytes.push(self.read_media(&id)?);
                }
                Ok(ResultData::Bytes(bytes))
            }
            Operation::Export(id, path) => {
                let bytes = self.read_media(&id)?;
                use std::io::Write;
                // The destination was explicitly selected with the native Save dialog.
                let mut options = OpenOptions::new();
                options.write(true).create(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options
                    .open(path)
                    .map_err(|_| "Cannot save attachment to that location")?;
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| "Cannot finish saving attachment")?;
                Ok(ResultData::Exported)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_identifiers_cannot_escape_staging_directory() {
        for bad in [
            "",
            ".",
            "..",
            "../../key",
            "/tmp/file",
            "C:\\file",
            "one/two",
            "one\\two",
        ] {
            assert!(engine_path(Path::new("engine"), bad).is_err());
        }
        assert_eq!(
            engine_path(Path::new("engine"), "abc-123.jpg").unwrap(),
            Path::new("engine/attachments/abc-123.jpg")
        );
    }
    #[test]
    fn imported_bytes_are_encrypted_and_removed_with_last_reference() {
        let dir = tempfile::tempdir().unwrap();
        private_dir(dir.path()).unwrap();
        let file = dir.path().join("private-name.txt");
        std::fs::write(&file, b"private attachment payload").unwrap();
        let lock = Store::lock(dir.path()).unwrap();
        let mut store = Store::initialize(dir.path(), "a", &[5; 32], "Test".into(), lock).unwrap();
        let ResultData::Imported(a) = store
            .media(Operation::Import(Source::File(file), Attachment::default()))
            .unwrap()
        else {
            panic!()
        };
        let id = a.media_id.clone().unwrap();
        let sealed: Vec<u8> = store
            .db
            .query_row("SELECT value FROM media", [], |r| r.get(0))
            .unwrap();
        assert!(!sealed.windows(7).any(|v| v == b"private"));
        store.save(&Snapshot::default()).unwrap(); // Pending import survives an older save.
        assert_eq!(
            &**store.read_media(&id).as_ref().unwrap(),
            b"private attachment payload"
        );
        let mut c = Conversation::new("peer".into(), "Peer".into(), false);
        c.draft_attachments.push(a);
        store
            .save(&Snapshot {
                conversations: vec![c],
                ..Default::default()
            })
            .unwrap();
        store.save(&Snapshot::default()).unwrap();
        assert!(store.read_media(&id).is_err());
    }
}
