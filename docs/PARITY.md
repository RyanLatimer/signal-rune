# Signal Desktop parity work

Reference: [Signal Desktop v8.34.0](https://github.com/signalapp/Signal-Desktop/releases/tag/v8.34.0), released 2026-11-05. This fixes the target instead of silently changing it as upstream releases new versions.

The user has confirmed that phone pairing and basic message exchange work. That is user-reported integration evidence, not an automated Android/iOS test on both platforms.

| Area | Current state | Required for completion |
| --- | --- | --- |
| Linking and account reuse | Implemented; pairing confirmed by user | Relink, revoke, expiry, multiple-account and migration tests |
| Direct/group text | Implemented; exchange confirmed by user | Full error/retry/rate-limit and network-loss coverage |
| Durable history | Authenticated-encrypted SQLite records, OS keychain keys, explicit Linux private-key-file fallback | Crash/recovery, large-history performance, backup/export/import audit |
| Outbox/reconnect | Commit-before-send, original-timestamp retries, automatic backoff | Service integration during disconnect and process failure |
| Phone sent/read sync | Implemented, replay/ordering/alias tests | Live cross-device group and direct tests |
| Typing | Send/start/refresh/stop, foreground gating, receive timeout, engine-enforced phone privacy | Live phone/client verification |
| Delivery/read receipts | Monotonic per-recipient state, foreground-only read and linked read sync | Live direct/group and privacy-setting verification |
| Conversation pins/archive | Durable local controls and ordering | Cross-device metadata synchronization and context menu parity |
| Disappearing/view-once messages | Read/send expiry, phone start-time sync, offline/restart purge and replay tombstones; view-once excluded | Timer controls and notices, live expiry tests, view-once media lifecycle |
| Replies/reactions/edit/delete | Native menus, quotes, edits/history, reactions, local/delete-for-everyone, durable retries, authenticated sync mutations | Live cross-device direct/group verification, full emoji picker |
| Attachments/media/voice notes | File picker/drop, encrypted cache, file-only send, PNG/JPEG previews and native Save dialog | Paste, full-size viewer, remaining media formats, playback/recording, live integration |
| Group management | Discovery and messaging | Members/admins, invites, requests, permissions, timer and link management |
| Identity/privacy | Engine trust failures fail closed | Safety numbers, verify/unverify, block/request UI, phone preference reflection |
| Calls | Engine exposes voice-call commands | Real call UI, audio devices/mute, video/group calls and screen sharing |
| Stories/stickers/polls | Not implemented yet | Full protocol and UI behavior |
| Desktop integration | Native window and keyboard editing | Notifications, tray, menus, shortcuts, window/zoom preferences, updates |
| Design/accessibility | Dark/light, named controls, native render matrix and contrast checks | Side-by-side v8.34.0 audit, all screens, scale settings, keyboard and screen readers |
| Distribution | Public repository and three native CI targets | Final-revision checks, launch on each OS, signed/notarized release and updater |

No stable release or completion claim is appropriate while required parity work remains open. Implemented features are also checked for real protocol behavior; visual fixtures alone do not establish that a feature works.

The bundled engine includes a small GPL extension with build-time tests for read-only phone privacy state, typing suppression, STOP behavior, and privacy-suppressed read-sync completion. Stock external engines fail closed for typing rather than guessing the phone setting.
