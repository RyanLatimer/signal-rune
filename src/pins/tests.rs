use super::*;
use crate::{storage, transport::Bridge};
use rune::Tx;
use std::{
    path::Path,
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
        assert!(Instant::now() < deadline, "Pin sync stalled: {:?}", s.error);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[allow(clippy::field_reassign_with_default, reason = "State flushes on drop")]
fn state(path: &Path) -> (State, Receiver<Value>) {
    let (bridge, requests, _) = Bridge::mock();
    let mut s = State::default();
    s.account = Some("self".into());
    s.account_aliases = vec!["self".into()];
    s.bridge = Some(bridge);
    s.store = Some(storage::Worker::test(path, "self"));
    pump(&mut s, |s| {
        s.history_ready && s.generation == s.saved_generation
    });
    if !s.conversations.iter().any(|c| c.id == "peer") {
        s.conversations
            .push(Conversation::new("peer".into(), "Peer".into(), false));
    }
    s.receive_pin_state(&json!({"supported":true,"ready":true,"pins":[],"count":0}));
    s.persist();
    pump(&mut s, |s| s.generation == s.saved_generation);
    (s, requests)
}
#[test]
fn pin_commit_queue_ack_restart_and_remote_ack_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, requests) = state(dir.path());
    for _ in requests.try_iter() {}
    s.toggle_pin("peer");
    let id = s.sync_ledger.pins[0].id;
    assert!(
        requests.try_recv().is_err(),
        "Engine write must wait for encrypted history commit"
    );
    pump(&mut s, |s| {
        s.pending.values().any(|p| matches!(p, Pending::Pin(_)))
    });
    let request = requests
        .try_iter()
        .find(|r| r["method"] == "setConversationPin")
        .unwrap();
    assert_eq!(request["params"]["operationId"], id);
    assert_eq!(request["params"]["recipient"], "peer");
    s.event(
        json!({"id":request["id"],"result":{"operationId":id,"synced":false}}),
        &mut Tx::default(),
    );
    assert_eq!(
        s.sync_ledger.pins.len(),
        1,
        "A queue ack must not be described as remote synchronization"
    );
    drop(s);
    let (mut s, requests) = state(dir.path());
    pump(&mut s, |s| {
        s.pending.values().any(|p| matches!(p, Pending::Pin(_)))
    });
    let retry = requests
        .try_iter()
        .find(|r| r["method"] == "setConversationPin")
        .unwrap();
    assert_eq!(retry["params"]["operationId"], id);
    s.receive_pin_state(&json!({"supported":true,"ready":true,"pins":[{"id":"peer","group":false}],"count":1,"completed":[id]}));
    assert!(s.sync_ledger.pins.is_empty());
    assert_eq!(
        s.conversations
            .iter()
            .find(|c| c.id == "peer")
            .unwrap()
            .pinned,
        1
    );
    s.persist();
    pump(&mut s, |s| s.generation == s.saved_generation);
}
#[test]
fn old_remote_ack_cannot_reverse_a_newer_unpin() {
    let mut s = State::default();
    s.preferences.pin_sync_initialized = true;
    s.conversations
        .push(Conversation::new("peer".into(), "Peer".into(), false));
    s.toggle_pin("peer");
    let old = s.sync_ledger.pins[0].id;
    s.toggle_pin("peer");
    s.receive_pin_state(&json!({"supported":true,"ready":true,"pins":[{"id":"peer","group":false}],"count":1,"completed":[old]}));
    assert_eq!(s.sync_ledger.pins.len(), 1);
    assert!(!s.sync_ledger.pins[0].pinned);
    assert_eq!(s.conversations[0].pinned, 0);
}
#[test]
fn migration_preserves_local_pin_order_and_waits_for_remote_storage() {
    let mut s = State::default();
    let mut first = Conversation::new("first".into(), "First".into(), false);
    first.pinned = 1;
    let mut second = Conversation::new("second".into(), "Second".into(), false);
    second.pinned = 2;
    s.conversations = vec![second, first];
    s.receive_pin_state(&json!({"supported":true,"ready":false,"pins":[]}));
    assert!(!s.preferences.pin_sync_initialized);
    assert!(s.sync_ledger.pins.is_empty());
    s.receive_pin_state(
        &json!({"supported":true,"ready":true,"pins":[{"id":"phone","group":false}],"count":1}),
    );
    assert_eq!(
        s.sync_ledger
            .pins
            .iter()
            .map(|p| p.conversation.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        s.conversations
            .iter()
            .any(|c| c.id == "phone" && c.pinned > 0)
    );
}
