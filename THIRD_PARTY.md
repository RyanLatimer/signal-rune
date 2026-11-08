# Third-party software

Signal Rune is an independent project, not an official Signal product. No Signal Desktop application source or artwork is included. The interface follows common Signal Desktop layouts. The application source is AGPL-3.0-only; see LICENSE.

- Rune and rune-kit: https://github.com/anomalyco/rune, MIT. Pinned source revision in Cargo.toml. Rune embeds fonts under their respective font licenses. rune-kit is the native equivalent of OpenCode's component system.
- signal-cli 0.14.10: https://github.com/AsamK/signal-cli, GPL-3.0. A separately executed runtime, with the small source-available GPL extension in `engine-extension/`. The extension adds read-only phone privacy settings and enforces typing privacy at command execution. It also treats privacy-suppressed read receipts as successful linked read syncs. The upstream release archive is SHA-256 verified before compiling the extension against its jars. Base archives are verified by the packaging script. Its lib directory includes libsignal and other dependencies under their own licenses. Source: https://github.com/AsamK/signal-cli/tree/v0.14.10.
- Eclipse Temurin Java 25: https://adoptium.net, GPLv2 with Classpath Exception and bundled third-party notices. Packaged from JAVA_HOME. Its legal directory is retained.
- qrcode: https://github.com/kennytm/qrcode-rust, MIT/Apache-2.0.
- directories: https://github.com/dirs-dev/directories-rs, MIT/Apache-2.0.
- serde and serde_json: https://github.com/serde-rs, MIT/Apache-2.0.
- Noto Color Emoji v2.051: https://github.com/googlefonts/noto-emoji/tree/8998f5dd683424a73e2314a8c1f1e359c19e8742, SIL Open Font License 1.1. The unmodified font is embedded so emoji remain available on machines without an installed emoji font. License: `licenses/NotoEmoji-OFL.txt`. The bundled Inter files come from the pinned Rune revision and use the existing Inter license in `licenses/`.

License texts for Rune, its embedded fonts and signal-cli are included in `licenses/` and copied into build archives.

Complete Rust dependency versions are in Cargo.lock. Runtime archives retain upstream files and license notices; the bundled Signal engine's source and build instructions are available at its versioned source link above.
