# Conversation pin synchronization

Pinning uses Signal's encrypted AccountRecord, including contact service IDs and group master-key identifiers. It does not send hidden chat messages. The client receives conversation IDs, never group master keys.

1. Rust saves each intent, its stable operation ID and the local pin state in encrypted history before sending an RPC.
2. The engine transaction records the operation, updates only the pinned-conversations field of the cached AccountRecord and rotates its storage ID. Other fields and unknown protobuf fields are preserved.
3. A queued operation is retried through the engine's existing serialized storage job executor. Pending intents are applied to freshly fetched remote pins during conflict reconciliation.
4. The engine acknowledges only a successful write of the current account storage ID, or a remote manifest proving that ID already exists. An older in-flight write cannot acknowledge a newer intent, because newer mutations rotate the ID again.
5. Rust retains its outbox after a queue acknowledgement and removes operations only after remote confirmation. A newer local unpin remains authoritative while an older acknowledgement arrives.

Completed operation IDs and their original request identities remain in the engine database for retry deduplication. A retry is checked before resolving a potentially changed phone number. Existing local pins migrate in their original order after remote account storage is ready. Missing storage state does not clear local pins.

The GPL extension contains pinned upstream copies of StorageHelper, StorageSyncModels and AccountRecordProcessor with narrow hooks into RunePins. Existing account privacy and unknown storage records remain under the upstream merge policy. Archive and mute metadata synchronization are separate, unfinished work.

Tests use real SQLite and Wire records without a Signal account or network. They establish local persistence and merge invariants; live Android/iOS synchronization remains a separate verification requirement.
