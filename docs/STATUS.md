# Parity and release status

This repository is a working first implementation, not the completed exact clone requested. No stable release should be tagged until the outstanding gates below pass.

## Implemented

- Native GPU-rendered Rust application, using pinned anomalyco/rune and rune-kit.
- Real Signal engine integration: multi-account-mode JSON-RPC over private stdio; selects the first linked account.
- QR provisioning (`startLink`, `finishLink`), expiry/error recovery, account reuse after restart.
- Contact and group discovery, contact/group synchronization request.
- Direct and group text send, incoming text and sent-message sync events.
- Pending/sent/failed delivery states (sent means accepted by the engine, not read by the recipient).
- Incoming replay deduplication, unread counts, conversation/message search, per-conversation drafts.
- Encrypted durable history and drafts, account isolation, authenticated records, atomic transactions and keychain integration. Linux reports its private-key-file fallback when no secret service is available.
- Durable outbox and automatic reconnect, original-timestamp retries, phone sent/read-state reconciliation and phone-number/ACI alias merging.
- Typing start/refresh/stop/timeout, delivery/read receipt state, foreground/read gating and phone-controlled receipt privacy.
- Durable pinned conversations and local archive controls.
- New recipient chat by international phone number or account UUID.
- Dark/light appearance, native keyboard-editable inputs, selectable message text, scrollable conversations.
- Settings, connection recovery and errors, accessible control names and keyboard shortcuts.
- Cross-platform build/portable bundle workflow: Linux x64, Apple Silicon macOS, Windows x64.

## Not implemented / not equivalent to Signal Desktop

- Phone history transfer and complete cross-device conversation metadata synchronization.
- Audio/video calls, screen sharing, voice recording/playback, stickers, stories and a full emoji picker.
- View-once support; its contents remain excluded. Disappearing read/send expiry is implemented and still needs live integration coverage.
- Key transparency, identity-change banners and message-request handling. Safety-number display, verify and unverify are implemented; failures never auto-trust identities.
- Desktop notifications, tray, auto-update, group avatars and complete group conflict UX.
- Full localization, exact icon/spacing parity, platform-specific menu integration, comprehensive accessibility audit.
- Signed/notarized installers, updater signing and release management.
- Full endpoint and metadata redaction audit, account migration, database recovery, linked account selection.

## Validation and remaining release gates

Automated tests cover message-envelope parsing, groups, numberless accounts, failed send semantics, provisioning expiry, replay deduplication, draft retention, and native pointer actions. The visual harness renders thirty-three states at 760×560, 1120×760 and 1440×900, saves PNGs/trees, checks named controls and bounds, and fails on Rune warnings. An independent OpenCV decoder reads the dense QR fixture from screenshots at all three sizes. Palette tests enforce at least 4.5:1 text contrast in both themes. This is layout evidence, not a pixel-exact comparison with official Signal Desktop.

The user has confirmed phone pairing and basic message exchange. The live cross-device cases in the parity matrix still require verification.

A live signal-cli 0.14.10 + Java 25 smoke test has verified `listAccounts` and a valid `sgnl://linkdevice` URI against the Signal service, using a separate disposable local directory. No user account or phone was accessed.

Still required before a stable release:

1. Successful build and engine smoke tests on all three CI runners; download and launch each actual packaged artifact on its OS.
2. Real Android and iOS linking approval with a test account; direct/group send/receive in both directions, phone sent-sync and reconnect/restart tests.
3. Expiry, network loss, identity-change, rate-limit and partial-send-failure testing against the service.
4. Side-by-side design audit against a specified official Signal Desktop version at multiple OS scaling settings; keyboard and screen-reader audits.
5. Implement missing features required for full parity, with secure persistence and message lifecycle handling first.
6. Resolve all material findings, sign/notarize installers, and only then publish a stable release.

The repository and CI artifacts can be public during development. They must not be presented as a verified final product.

## Local validation evidence (2026-11-07)

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --locked`: 13 tests passed.
- Native GPU screenshot harness: 30 renders passed, including independent decoding of the dense QR fixture at all three sizes.
- Manual PNG inspection: dark/light conversations, minimum-size linking and settings, QR fixture. Fixed small-window clipping and QR antialiasing seams during review.
- Text palette: 4.5:1 contrast assertions passed in both appearances.
- Linux release build and bundled Java/engine `--version` smoke test: passed.

The checked-in design report and representative screenshots use fictional data only. CI evidence belongs to each run's exact commit and is available as the `design-evidence` artifact. Neither automated layout checks nor these manual inspections establish exact visual parity with Signal Desktop.

The active, versioned feature inventory is [PARITY.md](PARITY.md). Current local validation: 66 Rust tests plus nine general engine checks and six pin-storage scenarios, strict Clippy, and 99 native renders. Full client parity remains in progress.

The Linux restart test exposed a retained lock descriptor; explicit release on history teardown and retained-descriptor/worker-restart regression tests now cover that case. The source-available engine extension enforces the phone typing setting at command execution and lets privacy-suppressed receipts complete their linked read sync.

Message-action and media checkpoint: durable sender-scoped edit/reaction/delete reduction, historical receipts, content-free replay tombstones, reply/edit draft recovery, multiline/IME handling, bundled color emoji, keyed history indexes with schema migration, encrypted media import/send/export and offline disappearing-message expiry. Tests cover a file-only send through actual SQLite import, restart, commit-before-send and blob deletion. Phone sync includes the original expiry start time through the GPL engine extension. Live device tests for these additions remain open.

Conversation/identity/group checkpoint: explicit safety-number comparison with scannable QR; verify and clear-verification operations bind the displayed number. Group creation, joining, roles, bans, requests, permissions, descriptions and links use actual engine commands. Changes wait for engine success and refreshed group state. Minimum-size tests scroll to controls below the fold. Contact alias reconciliation preserves deletion/expiry tombstones, queued-action IDs, edit drafts and receipt state. Edit retries retain their original wire timestamp. All three native CI packages passed for the preceding messaging/media checkpoint (`ed23eb4`).

Pin-sync checkpoint: local encrypted history commits before queueing an engine operation; an engine queue ack does not clear the outbox. Only a successful remote account-record write or a manifest containing the current unique storage ID acknowledges it. Tests cover restart, old acknowledgements, local-pin migration, unknown fields, unchanged phone privacy, concurrent remote pins and operation-ID reuse. History schema 3 prevents older builds from silently discarding the pin outbox. Live phone pin synchronization remains unverified.
