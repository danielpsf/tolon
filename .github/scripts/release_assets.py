#!/usr/bin/env python3
"""Release helpers for .github/workflows/release.yml (stdlib only; Python 3.10 on ubuntu-22.04).

  release_assets.py check-tag TAG             tag, versions and updater config agree
  release_assets.py prerelease TAG            print true when TAG is a pre-release (has a `-`), else false
  release_assets.py check-key SIG             a probe signature was made by the key in tauri.conf.json
  release_assets.py latest-json DIR TAG REPO  write DIR/latest.json for the Tauri updater
  release_assets.py stamp-install SRC OUT TAG write OUT: SRC (install.sh) with the version of TAG stamped in, so the
                                              copy attached to a release installs that release and no other
  release_assets.py notes TAG INSTALL_MD OUT [REPO]
                                              write the release notes: docs/releases/TAG.md, `---`, then INSTALL_MD with
                                              its relative links made absolute (REPO defaults to $GITHUB_REPOSITORY)

Tests: python3 -m unittest discover -s .github/scripts
"""
import base64
import fnmatch
import json
import os
import posixpath
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]
TAURI_CONF = ROOT / "crates/tolon-app/tauri.conf.json"
INSTALL_SH = ROOT / "install.sh"
# The one line of install.sh that holds the version. The script reads a value that starts with `@` as "not stamped".
INSTALL_STAMP = re.compile(r'^(  version=)"@TOLON_VERSION@"$', re.M)

# Updater manifest keys and the artifact patterns behind them, best first. The artifact is the file with a
# `.sig` next to it. Tauri 2 signs the installer itself (the AppImage, `-setup.exe`, `.msi`, `.deb`) and
# the macOS `.app.tar.gz`. Only those are offered: Tolon's updater is built without its `zip` feature
# (decision "Updater (L-019)"), so it cannot read the older `.nsis.zip`, `.msi.zip` or `.AppImage.tar.gz`
# archives. The updater plugin looks up `{os}-{arch}-{installer}` (windows-x86_64-nsis, linux-x86_64-deb, ...)
# before the plain `{os}-{arch}`, so a copy installed from an MSI or a deb gets its own file type back. The
# plain Windows and Linux keys point at NSIS and the AppImage. A pre-release has no MSI (WiX refuses its
# version, decision "Windows betas ship the NSIS installer only"), so its manifest leaves out the MSI key.
NSIS = ["*-setup.exe"]
MSI = ["*.msi"]
APPIMAGE = ["*.AppImage"]
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
NO_MSI_IN_PRERELEASE = {"windows-x86_64-msi"}


def is_prerelease(tag):
    return "-" in tag


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
    if len(INSTALL_STAMP.findall(INSTALL_SH.read_text())) != 1:
        errors.append('install.sh needs exactly one line `  version="@TOLON_VERSION@"` for the release to stamp (L-073)')
    return errors


def stamp_install(src, out, tag):
    """Write OUT: SRC with the version of TAG in place of the placeholder. Nothing is written on an error."""
    if not re.fullmatch(r"v\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?", tag):
        return [f"tag {tag!r} is not v<semver>"]
    text = Path(src).read_text()
    if len(INSTALL_STAMP.findall(text)) != 1:
        return [f'{src} needs exactly one line `  version="@TOLON_VERSION@"` to stamp; without it the copy would install the latest version, not {tag}']
    Path(out).write_text(INSTALL_STAMP.sub(rf'\g<1>"{tag.removeprefix("v")}"', text))
    return []


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
        if found is None and key in NO_MSI_IN_PRERELEASE and is_prerelease(tag):
            continue
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


def absolute_links(text, repo, tag):
    """Rewrite `](path)` targets, relative to docs/ (where install.md lives), to absolute URLs for TAG.

    Links go to blob/ and images to raw/ (blob/ pages do not render as an image in release notes)."""
    def fix(m):
        bang, label, target = m.groups()
        if re.match(r"([a-z][a-z0-9+.-]*:|#|/)", target, re.I):
            return m.group(0)
        path = posixpath.normpath(posixpath.join("docs", target))
        if path.startswith(".."):  # escapes the repo root: leave it
            return m.group(0)
        kind = "raw" if bang else "blob"
        return f"{bang}[{label}](https://github.com/{repo}/{kind}/{tag}/{path})"
    # shortcut: no support for links with a title or parentheses in the path; upgrade if install.md grows one.
    return re.sub(r"(!?)\[([^\]]*)\]\(([^)\s]*)\)", fix, text)


def notes(tag, install_md, out, repo):
    head = ROOT / "docs/releases" / f"{tag}.md"
    if not head.is_file():
        return [f"docs/releases/{tag}.md is missing: write the release notes before tagging"]
    body = absolute_links(Path(install_md).read_text(), repo, tag)
    Path(out).write_text(head.read_text().rstrip("\n") + "\n\n---\n\n" + body)
    return []


def main(argv):
    try:
        cmd, args = argv[1], argv[2:]
        if cmd == "check-tag" and len(args) == 1:
            errors = check_tag(args[0])
        elif cmd == "prerelease" and len(args) == 1:
            print("true" if is_prerelease(args[0]) else "false")
            errors = []
        elif cmd == "check-key" and len(args) == 1:
            errors = check_key(args[0])
        elif cmd == "latest-json" and len(args) == 3:
            latest_json(*args)
            errors = []
        elif cmd == "stamp-install" and len(args) == 3:
            errors = stamp_install(*args)
        elif cmd == "notes" and len(args) in (3, 4):
            repo = args[3] if len(args) == 4 else os.environ.get("GITHUB_REPOSITORY")
            errors = notes(*args[:3], repo) if repo else ["notes needs a repo: pass REPO or set GITHUB_REPOSITORY"]
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
