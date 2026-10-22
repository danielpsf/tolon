#!/bin/sh
# Installs Tolon on macOS:  curl -fsSL https://github.com/danielpsf/tolon/releases/latest/download/install.sh | sh
# What it does, and how to read it first: docs/install.md, "Install from the Terminal".
# The whole script is one function, called on the last line, so a download that is cut off runs nothing.
# The copy attached to a release is stamped with that release's version (release_assets.py stamp-install).
# The copy in the repository is not: it asks GitHub for the latest version instead.
# TOLON_APPLICATIONS replaces /Applications; the tests use it, so they never touch the real folder.

tolon_install() {
  set -eu

  repo="danielpsf/tolon"
  version="@TOLON_VERSION@"
  apps="${TOLON_APPLICATIONS:-/Applications}"

  say() { printf '%s\n' "$*"; }
  fail() { printf 'Tolon was not installed. %s\n' "$*" >&2; exit 1; }

  if [ "$(uname -s)" != "Darwin" ]; then
    printf 'This installer is for macOS. Download the installer for your system from https://github.com/%s/releases.\n' "$repo" >&2
    exit 1
  fi

  case "$version" in
    ""|@*)
      say "Looking up the latest version of Tolon."
      version=$(curl -fsSL "https://github.com/$repo/releases/latest/download/latest.json" |
        sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1) || version=""
      ;;
  esac
  printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' ||
    fail "Could not find out which version to install. Download it from https://github.com/$repo/releases."

  dmg="Tolon_${version}_universal.dmg"
  base="https://github.com/$repo/releases/download/v$version"

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/tolon-install.XXXXXX") || fail "Could not create a temporary folder."
  mounted=0
  trap 'if [ "$mounted" = 1 ]; then hdiutil detach -quiet "$tmp/mnt" >/dev/null 2>&1 || hdiutil detach -force -quiet "$tmp/mnt" >/dev/null 2>&1 || true; fi; rm -rf "$tmp"' EXIT
  trap 'exit 1' HUP INT TERM

  say "Downloading Tolon $version."
  curl -fsSL "$base/$dmg" -o "$tmp/$dmg" || fail "The download of $dmg failed. Check your connection and try again."
  curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "The download of SHA256SUMS failed. Check your connection and try again."

  awk -v f="$dmg" '$2 == f || $2 == "*" f' "$tmp/SHA256SUMS" > "$tmp/SHA256SUMS.dmg"
  [ -s "$tmp/SHA256SUMS.dmg" ] || fail "SHA256SUMS has no line for $dmg, so the download cannot be checked."
  (cd "$tmp" && shasum -a 256 -c SHA256SUMS.dmg >/dev/null 2>&1) ||
    fail "The checksum of $dmg does not match SHA256SUMS. The download was deleted. Try again, and report it if it keeps happening."
  say "The checksum matches."

  mkdir "$tmp/mnt"
  hdiutil attach -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/$dmg" >/dev/null </dev/null ||
    fail "Could not open $dmg."
  mounted=1
  [ -d "$tmp/mnt/Tolon.app" ] || fail "$dmg does not hold Tolon.app."

  if [ ! -d "$apps" ] || [ ! -w "$apps" ]; then
    apps="$HOME/Applications"
    mkdir -p "$apps" || fail "Could not create $apps."
  fi
  dest="$apps/Tolon.app"

  if [ -e "$dest" ] && pgrep -x tolon-app >/dev/null 2>&1; then
    say "Quitting Tolon so it can be replaced."
    pkill -x tolon-app >/dev/null 2>&1 || true
    waited=0
    while pgrep -x tolon-app >/dev/null 2>&1; do
      [ "$waited" -lt 20 ] || fail "Tolon is still running. Quit it from its menu bar icon, then run this command again."
      sleep 0.5
      waited=$((waited + 1))
    done
  fi

  say "Installing Tolon $version."
  staged="$apps/.Tolon.app.new"
  rm -rf "$staged"
  ditto "$tmp/mnt/Tolon.app" "$staged" || { rm -rf "$staged"; fail "Could not copy Tolon into $apps."; }
  rm -rf "$dest"
  mv "$staged" "$dest" || fail "Could not move Tolon into place in $apps."

  say "Installed Tolon $version in $dest."
  open "$dest" || say "Tolon did not open. Open it from $apps."
}

tolon_install "$@"
