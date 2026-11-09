#!/bin/sh
# Installs Tolon on macOS or Linux:  curl --proto '=https' --tlsv1.2 -fsSL https://github.com/danielpsf/tolon/releases/latest/download/install.sh | sh
# What it does, and how to read it first: docs/install.md, "Install from the Terminal".
# macOS: the .dmg into /Applications. Linux (x86_64): the AppImage into ~/.local/bin/tolon with a desktop entry.
# The whole script is one function, called on the last line, so a download that is cut off runs nothing.
# The copy attached to a release is stamped with that release's version (release_assets.py stamp-install).
# The copy in the repository is not: it asks GitHub for the latest version instead.
# TOLON_APPLICATIONS replaces /Applications; the tests use it, so they never touch the real folder.

tolon_install() {
  set -eu

  repo="danielpsf/tolon"
  version="@TOLON_VERSION@"
  apps="${TOLON_APPLICATIONS:-/Applications}"
  uid=$(id -u)

  say() { printf '%s\n' "$*"; }
  fetch() { curl --proto '=https' --tlsv1.2 -fsSL "$@"; }
  fail() { printf 'Tolon was not installed. %s\n' "$*" >&2; exit 1; }

  os=$(uname -s)
  case "$os" in
    Darwin|Linux) ;;
    *)
      printf 'This installer is for macOS and Linux. Download the installer for your system from https://github.com/%s/releases.\n' "$repo" >&2
      exit 1
      ;;
  esac
  [ "$uid" != 0 ] || fail "Run this command as your own user, not as root."
  if [ "$os" = Linux ] && [ "$(uname -m)" != x86_64 ]; then
    fail "Tolon has Linux builds for x86_64 only. See https://github.com/$repo/releases."
  fi

  case "$version" in
    ""|@*)
      say "Looking up the latest version of Tolon."
      version=$(fetch "https://github.com/$repo/releases/latest/download/latest.json" |
        sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1) || version=""
      ;;
  esac
  printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' ||
    fail "Could not find out which version to install. Download it from https://github.com/$repo/releases."

  if [ "$os" = Linux ]; then
    file="Tolon_${version}_amd64.AppImage"
    dest="$HOME/.local/bin/tolon"
  else
    file="Tolon_${version}_universal.dmg"
    dest="$apps/Tolon.app"
  fi
  base="https://github.com/$repo/releases/download/v$version"

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/tolon-install.XXXXXX") || fail "Could not create a temporary folder."
  mounted=0
  trap 'if [ "$mounted" = 1 ]; then hdiutil detach -quiet "$tmp/mnt" >/dev/null 2>&1 </dev/null || hdiutil detach -force -quiet "$tmp/mnt" >/dev/null 2>&1 </dev/null || true; fi; rm -rf "$tmp"' EXIT
  trap 'exit 1' HUP INT TERM

  say "Downloading Tolon $version."
  fetch "$base/$file" -o "$tmp/$file" || fail "The download of $file failed. Check your connection and try again."
  fetch "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "The download of SHA256SUMS failed. Check your connection and try again."

  awk -v f="$file" 'length($1) == 64 && ($2 == f || $2 == "*" f)' "$tmp/SHA256SUMS" > "$tmp/SHA256SUMS.one"
  [ -s "$tmp/SHA256SUMS.one" ] || fail "SHA256SUMS has no line for $file, so the download cannot be checked."
  # sha256sum is coreutils (every Linux); macOS has shasum instead.
  if command -v sha256sum >/dev/null 2>&1; then sum="sha256sum"; else sum="shasum -a 256"; fi
  (cd "$tmp" && $sum -c SHA256SUMS.one >/dev/null 2>&1) ||
    fail "The checksum of $file does not match SHA256SUMS. The download was deleted. Try again, and report it if it keeps happening."
  say "The checksum matches."

  if [ "$os" = Linux ]; then
    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications" || fail "Could not create $HOME/.local/bin."
  else
    mkdir "$tmp/mnt"
    hdiutil attach -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/$file" >/dev/null </dev/null ||
      fail "Could not open $file."
    mounted=1
    [ -d "$tmp/mnt/Tolon.app" ] || fail "$file does not hold Tolon.app."

    if [ ! -d "$apps" ] || [ ! -w "$apps" ]; then
      apps="$HOME/Applications"
      mkdir -p "$apps" || fail "Could not create $apps."
    fi
    dest="$apps/Tolon.app"
  fi

  if [ -e "$dest" ] && pgrep -U "$uid" -x tolon-app >/dev/null 2>&1; then
    say "Quitting Tolon so it can be replaced."
    pkill -U "$uid" -x tolon-app >/dev/null 2>&1 || true
    waited=0
    while pgrep -U "$uid" -x tolon-app >/dev/null 2>&1; do
      [ "$waited" -lt 20 ] || fail "Tolon is still running. Quit it from its menu bar icon, then run this command again."
      sleep 0.5
      waited=$((waited + 1))
    done
  fi

  say "Installing Tolon $version."
  if [ "$os" = Linux ]; then
    if ! cp "$tmp/$file" "$dest.new" || ! chmod 755 "$dest.new"; then
      rm -f "$dest.new"
      fail "Could not copy Tolon into $HOME/.local/bin."
    fi
    mv -f "$dest.new" "$dest" || { rm -f "$dest.new"; fail "Could not move Tolon into place in $HOME/.local/bin."; }
    printf '[Desktop Entry]\nType=Application\nName=Tolon\nComment=Never miss a meeting again.\nExec=%s\nTerminal=false\nCategories=Office;Calendar;\n' "$dest" \
      > "$HOME/.local/share/applications/tolon.desktop" || say "Could not write the applications menu entry. Start Tolon with $dest."
    say "Installed Tolon $version in $dest. It is in your applications menu as Tolon."
    "$dest" </dev/null >/dev/null 2>&1 &
    say "Tolon is starting. If it does not open, it needs the FUSE library (libfuse2 on Ubuntu); install it, then run $dest."
    return 0
  fi
  staged="$apps/.Tolon.app.new"
  rm -rf "$staged"
  ditto "$tmp/mnt/Tolon.app" "$staged" </dev/null || { rm -rf "$staged"; fail "Could not copy Tolon into $apps."; }
  rm -rf "$dest" || { rm -rf "$staged"; fail "Could not replace the Tolon in $apps. Delete it by hand, then run this command again."; }
  mv "$staged" "$dest" || fail "Could not move Tolon into place in $apps."

  say "Installed Tolon $version in $dest."
  open "$dest" </dev/null || say "Tolon did not open. Open it from $apps."
}

tolon_install "$@"
