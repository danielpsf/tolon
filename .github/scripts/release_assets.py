#!/usr/bin/env python3
"""Release helpers for .github/workflows/release.yml (stdlib only; Python 3.10 on ubuntu-22.04).

  release_assets.py check-tag TAG             tag, versions and updater config agree
  release_assets.py check-key SIG             a probe signature was made by the key in tauri.conf.json
  release_assets.py latest-json DIR TAG REPO  write DIR/latest.json for the Tauri updater

Tests: python3 -m unittest discover -s .github/scripts
"""
import base64
import fnmatch
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
TAURI_CONF = ROOT / "crates/tolon-app/tauri.conf.json"

# Updater manifest keys and the artifact patterns behind them, best first. The artifact is the file with a
# `.sig` next to it. Tauri 2 signs the installer itself (the AppImage, `-setup.exe`, `.msi`, `.deb`); the
# archive names are the older layout, kept as a fallback. The updater plugin looks up
# `{os}-{arch}-{installer}` (windows-x86_64-nsis, linux-x86_64-deb, ...) before the plain `{os}-{arch}`, so a
# copy installed from an MSI or a deb gets its own file type back. The plain Windows and Linux keys point at
# NSIS and the AppImage.
NSIS = ["*-setup.exe", "*-setup.nsis.zip"]
MSI = ["*.msi", "*.msi.zip"]
APPIMAGE = ["*.AppImage", "*.AppImage.tar.gz"]
APP = ["*.app.tar.gz"]
KEYS = {
    "darwin-aarch64": APP,
    "darwin-x86_64": APP,
    "windows-x86_64": NSIS + MSI,
    "windows-x86_64-nsis": NSIS,
    "windows-x86_64-msi": MSI,
    "linux-x86_64": APPIMAGE,
    "linux-x86_64-appimage": APPIMAGE,
    "linux-x86_64-deb": ["*.deb"],
}


def read_versions():
    conf = json.loads(TAURI_CONF.read_text())
    pkg = json.loads((ROOT / "ui/package.json").read_text())
    cargo = (ROOT / "Cargo.toml").read_text()
    m = re.search(r'^\[workspace\.package\][^\[]*?^version = "([^"]+)"', cargo, re.M | re.S)
    return conf, {
        "tauri.conf.json": conf.get("version"),
        "ui/package.json": pkg.get("version"),
        "Cargo.toml": m.group(1) if m else None,
    }


def check_tag(tag):
    errors = []
    if not re.fullmatch(r"v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?", tag):
        errors.append(f"tag {tag!r} is not v<semver>")
    conf, versions = read_versions()
    for name, version in versions.items():
        if version != tag.removeprefix("v"):
            errors.append(f"{name} says version {version!r}, the tag says {tag.removeprefix('v')!r}")
    if conf.get("bundle", {}).get("createUpdaterArtifacts") is not True:
        errors.append("bundle.createUpdaterArtifacts is not true in tauri.conf.json (L-019): no .sig files, no latest.json")
    updater = conf.get("plugins", {}).get("updater", {})
    if not updater.get("pubkey") or not updater.get("endpoints"):
        errors.append("plugins.updater needs a pubkey and endpoints in tauri.conf.json (L-019)")
    return errors


def key_id(minisign_b64):
    """The 8 key id bytes of a minisign public key or signature file (base64 of the whole file)."""
    text = base64.b64decode(minisign_b64).decode().splitlines()
    return base64.b64decode(text[1])[2:10]


def check_key(sig_path):
    conf, _ = read_versions()
    pub = conf["plugins"]["updater"]["pubkey"]
    if key_id(Path(sig_path).read_text().strip()) != key_id(pub):
        return ["TAURI_SIGNING_PRIVATE_KEY does not belong to the pubkey in tauri.conf.json: installed copies would reject every update"]
    return []


def latest_json(dist, tag, repo, now=None):
    dist = Path(dist)
    names = sorted(p.name for p in dist.iterdir() if p.is_file())
    platforms = {}
    for key, patterns in KEYS.items():
        found = next(
            (n for pat in patterns for n in names if fnmatch.fnmatch(n, pat) and f"{n}.sig" in names),
            None,
        )
        if found is None:
            raise SystemExit(f"no signed updater artifact for {key} in {dist} (looked for {patterns} with a .sig)")
        platforms[key] = {
            "signature": (dist / f"{found}.sig").read_text().strip(),
            "url": f"https://github.com/{repo}/releases/download/{tag}/{quote(found)}",
        }
    pub_date = (now or datetime.now(timezone.utc)).strftime("%Y-%m-%dT%H:%M:%SZ")
    manifest = {
        "version": tag.removeprefix("v"),
        "notes": f"https://github.com/{repo}/releases/tag/{tag}",
        "pub_date": pub_date,
        "platforms": platforms,
    }
    (dist / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def main(argv):
    try:
        cmd, args = argv[1], argv[2:]
        if cmd == "check-tag" and len(args) == 1:
            errors = check_tag(args[0])
        elif cmd == "check-key" and len(args) == 1:
            errors = check_key(args[0])
        elif cmd == "latest-json" and len(args) == 3:
            latest_json(*args)
            errors = []
        else:
            raise IndexError
    except IndexError:
        print(__doc__, file=sys.stderr)
        return 2
    for e in errors:
        print(f"::error::{e}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
