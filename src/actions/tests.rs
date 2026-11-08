use super::*;
use crate::{
    model::{Conversation, EditVersion},
    storage,
    transport::Bridge,
};
use std::{
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

#[allow(
    clippy::field_reassign_with_default,
    reason = "State has a flushing Drop implementation"
)]
fn state() -> (State, Receiver<Value>) {
    let (bridge, requests, _) = Bridge::mock();
    let mut s = State::default();
    s.account = Some("self".into());
    s.account_aliases = vec!["self".into()];
    s.bridge = Some(bridge);
    s.history_ready = true;
    s.configuration_due = 1000.;
    s.engine_configuration = Some(model::EngineConfiguration::default());
    s.conversations
        .push(Conversation::new("peer".into(), "Peer".into(), false));
    s.selected = Some("peer".into());
    (s, requests)
}
fn text(sender: &str, at: u64, body: &str) -> Value {
    json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":sender,"dataMessage":{"timestamp":at,"message":body}}}})
}
fn edit(sender: &str, target: u64, at: u64, body: &str) -> Value {
    json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":sender,"editMessage":{"targetSentTimestamp":target,"dataMessage":{"timestamp":at,"message":body}}}}})
}
fn reaction(target_author: &str, at: u64, emoji: &str, remove: bool) -> Value {
    json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":"peer","dataMessage":{"timestamp":at,"reaction":{"targetAuthorUuid":target_author,"targetSentTimestamp":100,"emoji":emoji,"isRemove":remove}}}}})
}
fn own(s: &mut State, at: u64) {
    s.conversations[0].messages.push(Message {
        id: 1,
        timestamp: at,
        body: "original".into(),
        sender: "self".into(),
        outgoing: true,
        read: true,
        delivery: Delivery::Sent,
        ..Message::default()
    });
}
fn pump(s: &mut State, condition: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition(s) {
        s.poll_history(&mut rune::Tx::default());
        s.poll_timers(&mut rune::Tx::default());
        assert!(
            Instant::now() < deadline,
            "Storage did not settle: {:?}",
            s.error
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn edit_chain_arriving_before_original_reconciles_and_keeps_history() {
    let (mut s, _) = state();
    s.receive(&edit("peer", 200, 300, "third"), &mut rune::Tx::default());
    s.receive(&edit("peer", 100, 200, "second"), &mut rune::Tx::default());
    s.receive(&text("peer", 100, "first"), &mut rune::Tx::default());
    let m = &s.conversations[0].messages[0];
    assert_eq!(m.body, "third");
    assert_eq!(m.latest_timestamp(), 300);
    assert_eq!(m.timestamp, 100);
    assert_eq!(
        m.edit_history
            .iter()
            .map(|v| v.body.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(s.conversations[0].unread, 1);
}
#[test]
fn forged_edit_cannot_modify_another_authors_message() {
    let (mut s, _) = state();
    own(&mut s, 100);
    s.receive(&edit("peer", 100, 200, "forged"), &mut rune::Tx::default());
    assert_eq!(s.conversations[0].messages[0].body, "original");
}
#[test]
fn reaction_replacement_and_removal_are_correct_before_original_arrives() {
    let (mut s, _) = state();
    s.receive(
        &reaction("self", 110, "👍", false),
        &mut rune::Tx::default(),
    );
    s.receive(
        &reaction("self", 120, "❤️", false),
        &mut rune::Tx::default(),
    );
    s.receive(&reaction("self", 130, "👍", true), &mut rune::Tx::default());
    own(&mut s, 100);
    s.apply_mutations();
    assert_eq!(s.conversations[0].messages[0].reactions["peer"], "❤️");
    s.receive(&reaction("self", 140, "❤️", true), &mut rune::Tx::default());
    assert!(s.conversations[0].messages[0].reactions.is_empty());
    s.receive(
        &reaction("self", 105, "👍", false),
        &mut rune::Tx::default(),
    );
    assert!(s.conversations[0].messages[0].reactions.is_empty());
    s.receive(
        &reaction("self", 150, "👍", false),
        &mut rune::Tx::default(),
    );
    assert_eq!(s.conversations[0].messages[0].reactions["peer"], "👍");
}
#[test]
fn remote_delete_does_not_resurrect_on_replay_or_late_edit() {
    let (mut s, _) = state();
    s.receive(
        &edit("peer", 100, 200, "edited secret"),
        &mut rune::Tx::default(),
    );
    s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","dataMessage":{"timestamp":300,"remoteDelete":{"timestamp":200}}}}}),&mut rune::Tx::default());
    s.receive(
        &text("peer", 100, "original secret"),
        &mut rune::Tx::default(),
    );
    let m = &s.conversations[0].messages[0];
    assert!(m.deleted);
    assert!(m.body.is_empty());
    assert!(m.edit_history.is_empty());
    s.receive(
        &text("peer", 100, "original secret"),
        &mut rune::Tx::default(),
    );
    s.receive(
        &edit("peer", 200, 400, "late secret"),
        &mut rune::Tx::default(),
    );
    assert_eq!(s.conversations[0].messages.len(), 1);
    assert!(s.conversations[0].messages[0].body.is_empty());
    assert!(
        !serde_json::to_string(&s.sync_ledger)
            .unwrap()
            .contains("secret")
    );
}
#[test]
fn edited_message_receipts_track_each_version_without_promoting_old_receipts() {
    let (mut s, _) = state();
    own(&mut s, 100);
    let receipt = |at| json!({"method":"receive","params":{"envelope":{"sourceUuid":"peer","receiptMessage":{"isRead":true,"timestamps":[at]}}}});
    s.receive(&receipt(100), &mut rune::Tx::default());
    assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Read);
    s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":"self","syncMessage":{"sentMessage":{"destinationUuid":"peer","editMessage":{"targetSentTimestamp":100,"dataMessage":{"timestamp":200,"message":"updated"}}}}}}}),&mut rune::Tx::default());
    assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Sent);
    assert_eq!(
        s.conversations[0].messages[0].edit_history[0].delivery,
        Delivery::Read
    );
    s.receive(&receipt(100), &mut rune::Tx::default());
    assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Sent);
    s.receive(&receipt(200), &mut rune::Tx::default());
    assert_eq!(s.conversations[0].messages[0].delivery, Delivery::Read);
}
#[test]
fn group_messages_with_same_timestamp_have_distinct_local_ids_and_authors() {
    let (mut s, _) = state();
    for author in ["alice", "bob"] {
        s.receive(&json!({"method":"receive","params":{"envelope":{"sourceUuid":author,"dataMessage":{"timestamp":100,"message":author,"groupInfo":{"groupId":"group"}}}}}),&mut rune::Tx::default());
    }
    let group = s.conversations.iter().find(|c| c.id == "group").unwrap();
    assert_eq!(group.messages.len(), 2);
    assert_ne!(group.messages[0].id, group.messages[1].id);
    let first = group.messages[0].id;
    assert_eq!(s.find_message("group", first).unwrap().sender, "alice");
}
#[test]
fn reply_and_edit_drafts_survive_selection_changes_and_cancel_restores_normal_draft() {
    let (mut s, _) = state();
    let at = model::now_ms() - 1000;
    own(&mut s, at);
    s.conversations
        .push(Conversation::new("other".into(), "Other".into(), false));
    s.composer.replace_text("unfinished normal draft");
    s.start_reply("peer", 1);
    assert_eq!(s.reply_to.as_ref().unwrap().timestamp, at);
    s.choose("other".into());
    s.choose("peer".into());
    assert_eq!(s.reply_to.as_ref().unwrap().timestamp, at);
    s.start_edit("peer", 1);
    assert_eq!(s.composer.text, "original");
    assert_eq!(s.editing, Some(1));
    s.choose("other".into());
    s.choose("peer".into());
    assert_eq!(s.editing, Some(1));
    s.cancel_edit();
    assert_eq!(s.composer.text, "unfinished normal draft");
    assert!(s.reply_to.is_some());
    assert!(s.editing.is_none());
}
#[test]
fn local_delete_preserves_only_replay_tombstone_and_discards_edit_content() {
    let (mut s, _) = state();
    s.receive(
        &text("peer", 100, "private original"),
        &mut rune::Tx::default(),
    );
    s.receive(
        &edit("peer", 100, 200, "private edit"),
        &mut rune::Tx::default(),
    );
    let id = s.conversations[0].messages[0].id;
    s.delete_for_me("peer", id);
    assert!(s.find_message("peer", id).is_none());
    s.receive(
        &text("peer", 100, "private original"),
        &mut rune::Tx::default(),
    );
    s.receive(
        &edit("peer", 200, 300, "new edit"),
        &mut rune::Tx::default(),
    );
    assert_eq!(s.conversations[0].messages.len(), 1);
    let m = &s.conversations[0].messages[0];
    assert!(m.hidden);
    assert!(m.body.is_empty());
    assert!(
        !serde_json::to_string(&s.conversations)
            .unwrap()
            .contains("private")
    );
    assert!(
        !serde_json::to_string(&s.sync_ledger)
            .unwrap()
            .contains("edit")
    );
}
#[test]
fn edit_limits_follow_24_hours_ten_edits_and_note_to_self_exception() {
    let mut m = Message {
        timestamp: model::now_ms() - 90_000_000,
        body: "old".into(),
        outgoing: true,
        delivery: Delivery::Sent,
        ..Message::default()
    };
    assert!(!m.can_edit(false));
    assert!(m.can_edit(true));
    assert!(!m.can_delete_for_everyone());
    m.edit_history = vec![EditVersion::default(); 10];
    assert!(!m.can_edit(true));
}
#[test]
fn read_ack_for_old_version_does_not_mark_new_edit_receipted() {
    let (mut s, _) = state();
    s.receive(&text("peer", 100, "old"), &mut rune::Tx::default());
    let id = s.conversations[0].messages[0].id;
    s.pending
        .insert(99, Pending::Receipt("peer".into(), vec![(id, 100)]));
    s.receive(&edit("peer", 100, 200, "new"), &mut rune::Tx::default());
    s.event(
        json!({"id":99,"result":{"timestamp":300,"results":[{"type":"SUCCESS"}]}}),
        &mut rune::Tx::default(),
    );
    assert!(!s.conversations[0].messages[0].read_receipt_sent);
}
#[test]
fn durable_action_waits_for_commit_and_uses_current_target_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, requests) = state();
    let at = model::now_ms() - 1000;
    own(&mut s, at);
    s.history_ready = false;
    s.store = Some(storage::Worker::test(dir.path(), "self"));
    pump(&mut s, |s| {
        s.history_ready && s.generation > 0 && s.saved_generation == s.generation
    });
    for _ in requests.try_iter() {}
    assert!(s.queue_action(
        "peer",
        1,
        ActionKind::Reaction {
            emoji: "👍".into(),
            remove: false
        }
    ));
    assert!(requests.try_recv().is_err());
    pump(&mut s, |s| {
        s.pending.values().any(|p| matches!(p, Pending::Action(..)))
    });
    let sent = requests
        .try_iter()
        .find(|r| r["method"] == "sendReaction")
        .unwrap();
    assert_eq!(sent["params"]["targetTimestamp"], at);
    assert_eq!(sent["params"]["targetAuthor"], "self");
    assert_eq!(sent["params"]["emoji"], "👍");
}
#[test]
fn phone_note_to_self_sync_has_no_destination_but_still_routes_correctly() {
    let (mut s, _) = state();
    s.receive(&json!({"method":"receive","params":{"account":"self","envelope":{"sourceUuid":"self","syncMessage":{"sentMessage":{"timestamp":100,"message":"note"}}}}}),&mut rune::Tx::default());
    let notes = s.conversations.iter().find(|c| c.id == "self").unwrap();
    assert_eq!(notes.name, "Note to Self");
    assert_eq!(notes.messages[0].body, "note");
    assert!(notes.messages[0].outgoing);
}
