use super::*;
use crate::{model::Conversation, storage, transport::Bridge};
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
fn pump(s: &mut State, condition: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !condition(s) {
        s.poll_history(&mut Tx::default());
        s.poll_timers(&mut Tx::default());
        if s.dirty {
            s.persist();
        }
        assert!(
            Instant::now() < deadline,
            "media pipeline stalled: {:?}",
            s.error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
fn state(dir: &std::path::Path) -> (State, Receiver<serde_json::Value>) {
    let (bridge, requests, _) = Bridge::mock();
    let mut s = State::default();
    s.account = Some("self".into());
    s.account_aliases = vec!["self".into()];
    s.bridge = Some(bridge);
    s.store = Some(storage::Worker::test(dir, "self"));
    s.conversations
        .push(Conversation::new("peer".into(), "Peer".into(), false));
    pump(&mut s, |s| {
        s.history_ready && s.generation == s.saved_generation
    });
    s.selected = Some("peer".into());
    (s, requests)
}
#[test]
fn filenames_and_inline_data_cannot_inject_uri_parameters() {
    assert_eq!(safe_name("../../report\n.txt"), "report.txt");
    let a = Attachment {
        name: "a+b;filename=evil, 😃.txt".into(),
        content_type: "text/plain\r\nleak".into(),
        ..Default::default()
    };
    let uri = data_uri(&a, b"payload");
    assert!(uri.starts_with("data:application/octet-stream;filename=a%2Bb%3Bfilename%3Devil%2C%20%F0%9F%98%83.txt;base64,"));
    assert_eq!(
        STANDARD
            .decode(uri.split_once(";base64,").unwrap().1)
            .unwrap(),
        b"payload"
    );
}
#[test]
fn file_only_send_commits_first_survives_restart_and_deletes_encrypted_blob() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, b"attachment secret").unwrap();
    let (mut s, requests) = state(dir.path());
    s.import_files("peer", std::slice::from_ref(&file));
    pump(&mut s, |s| {
        s.conversations
            .iter()
            .find(|c| c.id == "peer")
            .unwrap()
            .draft_attachments
            .len()
            == 1
            && s.generation == s.saved_generation
    });
    let media_id = s
        .conversations
        .iter()
        .find(|c| c.id == "peer")
        .unwrap()
        .draft_attachments[0]
        .media_id
        .clone()
        .unwrap();
    assert!(file.exists(), "Original user-picked file is preserved");
    drop(s);
    let (mut s, requests2) = state(dir.path());
    assert_eq!(
        s.conversations
            .iter()
            .find(|c| c.id == "peer")
            .unwrap()
            .draft_attachments
            .len(),
        1
    );
    s.send(&mut Tx::default());
    assert!(
        !requests
            .try_iter()
            .chain(requests2.try_iter())
            .any(|v| v["method"] == "send")
    );
    pump(&mut s, |s| {
        s.pending.values().any(|p| matches!(p, Pending::Send(..)))
    });
    let send = requests2
        .try_iter()
        .find(|v| v["method"] == "send")
        .unwrap();
    assert_eq!(send["params"]["message"], "");
    assert_eq!(
        send["params"]["attachment"][0],
        "data:text/plain;filename=note.txt;base64,YXR0YWNobWVudCBzZWNyZXQ="
    );
    let id = s
        .conversations
        .iter()
        .find(|c| c.id == "peer")
        .unwrap()
        .messages[0]
        .id;
    s.delete_for_me("peer", id);
    pump(&mut s, |s| s.generation == s.saved_generation);
    let db = rusqlite::Connection::open(dir.path().join("history.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM media WHERE id=?1", [media_id], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap(),
        0
    );
    s.flush_outbox();
    assert!(!requests2.try_iter().any(|v| v["method"] == "send"));
}
#[test]
fn parses_attachment_only_messages_and_keeps_view_once_excluded() {
    let mut v = json!({"params":{"envelope":{"sourceUuid":"peer","dataMessage":{"timestamp":100,"attachments":[{"id":"safe.jpg","filename":"../photo.jpg","contentType":"image/jpeg","size":99}]}}}});
    let m = crate::model::incoming(&v).unwrap();
    assert!(m.body.is_empty());
    assert_eq!(m.attachments[0].name, "photo.jpg");
    v["params"]["envelope"]["dataMessage"]["viewOnce"] = json!(true);
    assert!(crate::model::incoming(&v).is_none());
}
