# Release agent

Owns `.github/workflows/`, `docs/install.md`, the updater and the release checklist.

- `ci.yml` runs the gate on ubuntu-22.04, windows-latest and macos-latest on every PR; cache cargo and npm.
- `release.yml` triggers on tags `v*`. Job `preflight` checks the secrets, the tag against every version, `cargo xtask check-release-features` and that the signing key matches the pubkey. Job `build` runs the Tauri CLI per OS (not `tauri-action`, so only `publish` needs `contents: write`): macOS universal DMG with `APPLE_SIGNING_IDENTITY: "-"` (ad-hoc), Windows MSI + NSIS (NSIS only for a pre-release version, because WiX refuses a non-numeric pre-release; the `prerelease` output of `preflight` decides), Linux AppImage + deb. Job `publish` writes `latest.json` and `SHA256SUMS` (`.github/scripts/release_assets.py`) and creates a draft release whose body is `docs/install.md`. A person promotes the draft. It is not marked pre-release, because the updater endpoint is `releases/latest/download/latest.json` and GitHub's "latest" skips pre-releases.
- Release notes: `docs/releases/<tag>.md` is required (preflight fails without it) and comes first in the draft body, then `---`, then `docs/install.md`. The head file must use absolute links (`https://github.com/<repo>/blob/<tag>/...`), because only install.md is rewritten (relative links in it are read from `docs/`; images become `raw/` URLs).
- Updater keys: generate once with `npm run tauri signer generate`; the public key goes in `tauri.conf.json`, the private key and password into repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit the private key.
- Version bump: `cargo xtask bump 0.1.0-beta.2` updates `Cargo.toml`, `tauri.conf.json`, `ui/package.json`, `CHANGELOG.md`.
- Before tagging: the QA agent has signed the manual checklist for all three OS in the ledger.
- Optional later: Windows signing through SignPath Foundation (free for open source).
