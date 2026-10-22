//! Tests for `install.sh` (L-073), part of `cargo test --workspace` and so of `cargo xtask ci`.
//!
//! The script runs under `sh` with stub `uname`, `curl`, `hdiutil`, `ditto`, `open`, `pkill` and `pgrep` first on
//! PATH. Each stub logs its call to one file. HOME, TMPDIR and the Applications folder (`TOLON_APPLICATIONS`) are
//! temporary folders, so a test never touches the real /Applications or a real Tolon. Unix only: Windows has no `sh`
//! on the runner's PATH that the script could use, and the script is macOS only anyway.
#![cfg(unix)]

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const VERSION: &str = "1.2.3";
const DMG: &str = "Tolon_1.2.3_universal.dmg";

fn repo_script() -> Result<PathBuf> {
    Ok(crate::repo_root()?.join("install.sh"))
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new() -> Result<Self> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "tolon-install-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let sb = Sandbox { root };
        for dir in ["stubs", "server", "home", "tmp"] {
            fs::create_dir_all(sb.path(dir))?;
        }
        for (name, body) in STUBS {
            let file = sb.path("stubs").join(name);
            fs::write(&file, format!("#!/bin/sh\n{body}\n"))?;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
        }
        fs::write(sb.path("server").join(DMG), "dmg bytes")?;
        sb.sums()?;
        Ok(sb)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// SHA256SUMS of the server folder as it is now, written by the real `shasum`.
    fn sums(&self) -> Result<()> {
        let status = Command::new("sh")
            .current_dir(self.path("server"))
            .args(["-c", "shasum -a 256 Tolon_*.dmg > SHA256SUMS"])
            .status()
            .context("cannot run shasum")?;
        anyhow::ensure!(status.success(), "shasum failed");
        Ok(())
    }

    /// `install.sh` with the release's stamp, as release_assets.py writes it.
    fn stamped(&self) -> Result<PathBuf> {
        let text = fs::read_to_string(repo_script()?)?;
        let stamped = text.replace(
            "version=\"@TOLON_VERSION@\"",
            &format!("version=\"{VERSION}\""),
        );
        anyhow::ensure!(stamped != text, "install.sh has no stamp line");
        let out = self.path("install.sh");
        fs::write(&out, stamped)?;
        Ok(out)
    }

    fn run(&self, script: &Path, apps: &Path, extra: &[(&str, &str)]) -> Result<Output> {
        let path = format!(
            "{}:{}",
            self.path("stubs").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut cmd = Command::new("sh");
        cmd.arg(script)
            .env("PATH", path)
            .env("HOME", self.path("home"))
            .env("TMPDIR", self.path("tmp"))
            .env("TOLON_APPLICATIONS", apps)
            .env("LOG", self.path("log"))
            .env("SERVER", self.path("server"))
            .env("STUBS", self.path("stubs"));
        for (k, v) in extra {
            cmd.env(k, v);
        }
        cmd.output().context("cannot run sh")
    }

    fn log(&self) -> String {
        fs::read_to_string(self.path("log")).unwrap_or_default()
    }

    fn tmp_is_empty(&self) -> Result<bool> {
        Ok(fs::read_dir(self.path("tmp"))?.next().is_none())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn position(log: &str, needle: &str) -> Option<usize> {
    log.lines().position(|l| l.contains(needle))
}

const STUBS: &[(&str, &str)] = &[
    ("uname", r#"echo "${STUB_UNAME:-Darwin}""#),
    (
        "curl",
        r#"echo "curl $*" >> "$LOG"
out=""; url=""
while [ $# -gt 0 ]; do
  case "$1" in -o) out="$2"; shift ;; -*) ;; *) url="$1" ;; esac
  shift
done
f="$SERVER/${url##*/}"
[ -f "$f" ] || exit 22
if [ -n "$out" ]; then cp "$f" "$out"; else cat "$f"; fi"#,
    ),
    (
        "hdiutil",
        r#"echo "hdiutil $*" >> "$LOG"
if [ "$1" = attach ]; then
  mp=""
  while [ $# -gt 0 ]; do [ "$1" = -mountpoint ] && mp="$2"; shift; done
  mkdir -p "$mp/Tolon.app/Contents/MacOS"
  echo new > "$mp/Tolon.app/Contents/MacOS/tolon-app"
fi"#,
    ),
    ("ditto", r#"echo "ditto $*" >> "$LOG"; cp -R "$1" "$2""#),
    ("open", r#"echo "open $*" >> "$LOG""#),
    (
        "pkill",
        r#"echo "pkill $*" >> "$LOG"; rm -f "$STUBS/running""#,
    ),
    ("pgrep", r#"[ -f "$STUBS/running" ]"#),
];

fn code_lines(script: &str) -> Vec<&str> {
    script
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect()
}

#[test]
fn syntax_is_posix_sh_and_shellcheck_is_clean_when_installed() -> Result<()> {
    let script = repo_script()?;
    let status = Command::new("sh").arg("-n").arg(&script).status()?;
    assert!(status.success(), "sh -n install.sh failed");
    match Command::new("shellcheck").arg("--version").output() {
        Ok(o) if o.status.success() => {
            let out = Command::new("shellcheck")
                .args(["-s", "sh"])
                .arg(&script)
                .output()?;
            assert!(out.status.success(), "shellcheck: {}", stdout(&out));
        }
        _ => println!("skipped: shellcheck is not installed"),
    }
    Ok(())
}

#[test]
fn the_whole_body_is_one_function_called_on_the_last_line_only() -> Result<()> {
    let text = fs::read_to_string(repo_script()?)?;
    let top: Vec<&str> = code_lines(&text)
        .into_iter()
        .filter(|l| !l.starts_with(char::is_whitespace))
        .collect();
    assert_eq!(top, ["tolon_install() {", "}", "tolon_install \"$@\""]);
    assert_eq!(text.lines().last(), Some("tolon_install \"$@\""));
    Ok(())
}

#[test]
fn never_uses_sudo_or_apple_events() -> Result<()> {
    let text = fs::read_to_string(repo_script()?)?;
    for line in code_lines(&text) {
        for word in ["sudo", "osascript", "tell application"] {
            assert!(!line.contains(word), "{word} in: {line}");
        }
    }
    Ok(())
}

#[test]
fn refuses_to_run_off_macos_before_any_download() -> Result<()> {
    let sb = Sandbox::new()?;
    let apps = sb.path("apps");
    let out = sb.run(&sb.stamped()?, &apps, &[("STUB_UNAME", "Linux")])?;
    assert!(!out.status.success());
    assert!(stderr(&out).contains("macOS"), "{}", stderr(&out));
    assert!(stderr(&out).contains("https://github.com/danielpsf/tolon/releases"));
    assert_eq!(sb.log(), "", "nothing may run before the check");
    Ok(())
}

#[test]
fn a_checksum_mismatch_stops_before_any_copy_and_cleans_up() -> Result<()> {
    let sb = Sandbox::new()?;
    fs::write(sb.path("server").join(DMG), "tampered bytes")?;
    let apps = sb.path("apps");
    fs::create_dir(&apps)?;
    let out = sb.run(&sb.stamped()?, &apps, &[])?;
    assert!(!out.status.success());
    assert!(stderr(&out).contains("checksum"), "{}", stderr(&out));
    let log = sb.log();
    assert!(!log.contains("ditto"), "ditto ran: {log}");
    assert!(!log.contains("hdiutil"), "the image was opened: {log}");
    assert!(!log.contains("open "), "Tolon was opened: {log}");
    assert_eq!(fs::read_dir(&apps)?.count(), 0);
    assert!(sb.tmp_is_empty()?);
    Ok(())
}

#[test]
fn a_missing_checksum_line_stops_before_any_copy() -> Result<()> {
    let sb = Sandbox::new()?;
    fs::write(
        sb.path("server/SHA256SUMS"),
        format!("{}  other.dmg\n", "0".repeat(64)),
    )?;
    let out = sb.run(&sb.stamped()?, &sb.path("apps"), &[])?;
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no line for"), "{}", stderr(&out));
    assert!(!sb.log().contains("ditto"));
    assert!(sb.tmp_is_empty()?);
    Ok(())
}

#[test]
fn installs_into_home_applications_when_the_system_folder_is_not_writable() -> Result<()> {
    let sb = Sandbox::new()?;
    let out = sb.run(&sb.stamped()?, &sb.path("no-such-folder"), &[])?;
    assert!(out.status.success(), "{}", stderr(&out));
    let app = sb.path("home/Applications/Tolon.app");
    assert!(app.join("Contents/MacOS/tolon-app").is_file());
    assert!(stdout(&out).contains(&format!("Installed Tolon {VERSION} in {}", app.display())));
    assert!(!stdout(&out).contains('!'), "no exclamation marks");
    let log = sb.log();
    let url = format!(
        "curl -fsSL https://github.com/danielpsf/tolon/releases/download/v{VERSION}/{DMG} -o "
    );
    let order: Vec<Option<usize>> = [
        &url,
        "SHA256SUMS",
        "hdiutil attach -nobrowse -readonly -mountpoint ",
        "ditto ",
        &format!("open {}", app.display()),
        "hdiutil detach",
    ]
    .iter()
    .map(|n| position(&log, n))
    .collect();
    assert!(
        order.iter().all(Option::is_some),
        "missing a call in: {log}"
    );
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "calls out of order: {log}"
    );
    assert!(!log.contains("pkill"), "nothing was running");
    assert!(sb.tmp_is_empty()?);
    Ok(())
}

#[test]
fn installs_into_the_system_folder_when_it_is_writable() -> Result<()> {
    let sb = Sandbox::new()?;
    let apps = sb.path("apps");
    fs::create_dir(&apps)?;
    let out = sb.run(&sb.stamped()?, &apps, &[])?;
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(apps.join("Tolon.app/Contents/MacOS/tolon-app").is_file());
    assert!(!sb.path("home/Applications").exists());
    assert!(
        fs::read_dir(&apps)?.count() == 1,
        "no staging folder is left behind"
    );
    Ok(())
}

#[test]
fn a_running_tolon_is_quit_before_it_is_replaced() -> Result<()> {
    let sb = Sandbox::new()?;
    let apps = sb.path("apps");
    fs::create_dir_all(apps.join("Tolon.app/Contents/MacOS"))?;
    fs::write(apps.join("Tolon.app/Contents/MacOS/tolon-app"), "old")?;
    fs::write(apps.join("Tolon.app/stale-file"), "old")?;
    fs::write(sb.path("stubs/running"), "")?;
    let out = sb.run(&sb.stamped()?, &apps, &[])?;
    assert!(out.status.success(), "{}", stderr(&out));
    let log = sb.log();
    assert!(log.contains("pkill -x tolon-app"), "{log}");
    assert!(position(&log, "pkill") < position(&log, "ditto"), "{log}");
    assert_eq!(
        fs::read_to_string(apps.join("Tolon.app/Contents/MacOS/tolon-app"))?,
        "new\n"
    );
    assert!(
        !apps.join("Tolon.app/stale-file").exists(),
        "the old copy is replaced, not merged"
    );
    Ok(())
}

#[test]
fn the_repository_copy_asks_for_the_latest_version() -> Result<()> {
    let sb = Sandbox::new()?;
    fs::write(
        sb.path("server/latest.json"),
        format!("{{\n  \"version\": \"{VERSION}\",\n  \"platforms\": {{\n    \"x\": {{ \"version\": \"0.0.0\" }}\n  }}\n}}\n"),
    )?;
    let out = sb.run(&repo_script()?, &sb.path("apps"), &[])?;
    assert!(out.status.success(), "{}", stderr(&out));
    let log = sb.log();
    assert!(
        log.contains("releases/latest/download/latest.json"),
        "{log}"
    );
    assert!(
        log.contains(&format!("releases/download/v{VERSION}/{DMG}")),
        "{log}"
    );
    Ok(())
}

#[test]
fn without_a_version_it_stops_before_downloading_an_installer() -> Result<()> {
    let sb = Sandbox::new()?;
    let out = sb.run(&repo_script()?, &sb.path("apps"), &[])?;
    assert!(!out.status.success());
    assert!(stderr(&out).contains("which version"), "{}", stderr(&out));
    assert!(!sb.log().contains(".dmg"));
    Ok(())
}
