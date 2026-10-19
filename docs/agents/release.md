# Release agent

Owns `.github/workflows/`, `docs/install.md`, the updater and the release checklist.

- `ci.yml` runs the gate on ubuntu-22.04, windows-latest and macos-latest on every PR; cache cargo and npm.
- `release.yml` triggers on tags `v*` and uses `tauri-apps/tauri-action` to build: macOS universal DMG with `APPLE_SIGNING_IDENTITY: "-"` (ad-hoc), Windows MSI + NSIS, Linux AppImage + deb; uploads `latest.json` for the updater and a `SHA256SUMS` file; release body = `docs/install.md`.
- Updater keys: generate once with `npm run tauri signer generate`; the public key goes in `tauri.conf.json`, the private key and password into repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit the private key.
- Version bump: `cargo xtask bump 0.1.0-beta.2` updates `Cargo.toml`, `tauri.conf.json`, `ui/package.json`, `CHANGELOG.md`.
- Before tagging: the QA agent has signed the manual checklist for all three OS in the ledger.
- Optional later: Windows signing through SignPath Foundation (free for open source).
