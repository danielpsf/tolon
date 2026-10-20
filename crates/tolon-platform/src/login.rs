//! Start at login (AC9.4): the pure parts, free of OS calls so every OS tests them. The macOS,
//! Windows and Linux implementations feed them the real OS state; nothing here is cached.

use crate::PlatformError;
use std::path::{Path, PathBuf};

/// Whether Tolon starts when the person logs in, as the OS reports it right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartAtLogin {
    Enabled,
    Disabled,
    /// macOS only: Tolon is registered but the person has not allowed it yet. The switch shows
    /// off and the UI says: "Allow Tolon in System Settings, Login Items."
    RequiresApproval,
}

impl StartAtLogin {
    /// The state the switch shows: on only when the OS will really start Tolon.
    pub fn is_enabled(self) -> bool {
        self == StartAtLogin::Enabled
    }

    /// Stable code for logs and the UI; the UI turns it into copy.
    pub fn code(self) -> &'static str {
        match self {
            StartAtLogin::Enabled => "enabled",
            StartAtLogin::Disabled => "disabled",
            StartAtLogin::RequiresApproval => "requires_approval",
        }
    }
}

/// macOS `SMAppServiceStatus` (raw `NSInteger`) to a state: 0 not registered, 1 enabled,
/// 2 requires approval, 3 not found. Not found (a build outside an app bundle, such as
/// `cargo run`) and any unknown value read as disabled: the OS will not start Tolon.
pub fn from_sm_app_service_status(raw: isize) -> StartAtLogin {
    match raw {
        1 => StartAtLogin::Enabled,
        2 => StartAtLogin::RequiresApproval,
        _ => StartAtLogin::Disabled,
    }
}

/// The `Run` value for the current exe: the path in double quotes, so a path with spaces
/// (`C:\Program Files\Tolon\Tolon.exe`) is not split at the first space.
pub fn windows_run_value(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Whether a `Run` value starts `exe`. Windows paths ignore case. An unquoted value, or a
/// quoted one followed by arguments, still counts when its program is `exe`.
pub fn windows_run_value_is(value: &str, exe: &str) -> bool {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => value,
    };
    program.eq_ignore_ascii_case(exe)
}

/// One `Exec` argument as the Desktop Entry spec wants it: `%` doubled, and a double-quoted
/// string with `"`, `` ` ``, `$` and `\` backslash-escaped when it holds a reserved character.
fn exec_arg(arg: &str) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let arg = arg.replace('%', "%%");
    if !arg.is_empty() && !arg.contains(|c| RESERVED.contains(c)) {
        return arg;
    }
    let mut quoted = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// The `Exec=` value for a program path: [`exec_arg`], then backslashes doubled because the
/// key file format reads `\\` as one backslash in a string value.
fn exec_line(exe: &str) -> String {
    exec_arg(exe).replace('\\', "\\\\")
}

/// The autostart `.desktop` file for `exe`. `None` when the path holds a line break, which a
/// key file value cannot carry.
pub fn desktop_entry(exe: &str) -> Option<String> {
    if exe.contains(['\n', '\r']) {
        return None;
    }
    Some(format!(
        "[Desktop Entry]\nType=Application\nName=Tolon\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_line(exe)
    ))
}

/// Whether `contents` is an enabled autostart entry that starts `exe`: same `Exec`, and neither
/// `Hidden=true` nor `X-GNOME-Autostart-enabled=false`.
pub fn desktop_entry_enables(contents: &str, exe: &str) -> bool {
    let value = |key: &str| {
        contents
            .lines()
            .find_map(|l| l.trim().strip_prefix(key)?.trim_start().strip_prefix('='))
            .map(str::trim)
    };
    value("Exec") == Some(exec_line(exe).as_str())
        && value("Hidden") != Some("true")
        && value("X-GNOME-Autostart-enabled") != Some("false")
}

/// `<config home>/autostart/dev.tolon.desktop`. The config home is `XDG_CONFIG_HOME` when it is
/// an absolute path (the spec ignores a relative one), else `$HOME/.config`.
pub fn autostart_file(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let config = match xdg_config_home.map(Path::new) {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        _ => Path::new(home.filter(|h| !h.is_empty())?).join(".config"),
    };
    Some(config.join("autostart").join("dev.tolon.desktop"))
}

fn io(e: std::io::Error) -> PlatformError {
    PlatformError::Io(e.to_string())
}

/// The state of the autostart file at `file` for `exe`; a missing file is disabled.
pub fn read_autostart(file: &Path, exe: &str) -> Result<StartAtLogin, PlatformError> {
    match std::fs::read_to_string(file) {
        Ok(c) if desktop_entry_enables(&c, exe) => Ok(StartAtLogin::Enabled),
        Ok(_) => Ok(StartAtLogin::Disabled),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StartAtLogin::Disabled),
        Err(e) => Err(io(e)),
    }
}

/// Create or replace the autostart file at `file` so it starts `exe`.
pub fn write_autostart(file: &Path, exe: &str) -> Result<(), PlatformError> {
    let entry = desktop_entry(exe)
        .ok_or_else(|| PlatformError::Io("the program path holds a line break".into()))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(file, entry).map_err(io)
}

/// Delete the autostart file; one that is already gone is fine.
pub fn remove_autostart(file: &Path) -> Result<(), PlatformError> {
    match std::fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tolon-login-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn ac9_4_macos_status_maps_to_a_state() {
        assert_eq!(from_sm_app_service_status(0), StartAtLogin::Disabled);
        assert_eq!(from_sm_app_service_status(1), StartAtLogin::Enabled);
        assert_eq!(
            from_sm_app_service_status(2),
            StartAtLogin::RequiresApproval
        );
        assert_eq!(from_sm_app_service_status(3), StartAtLogin::Disabled);
        assert_eq!(from_sm_app_service_status(99), StartAtLogin::Disabled);
    }

    #[test]
    fn ac9_4_only_enabled_turns_the_switch_on() {
        assert!(StartAtLogin::Enabled.is_enabled());
        assert!(!StartAtLogin::Disabled.is_enabled());
        assert!(!StartAtLogin::RequiresApproval.is_enabled());
        assert_eq!(StartAtLogin::RequiresApproval.code(), "requires_approval");
    }

    #[test]
    fn ac9_4_windows_run_value_quotes_a_path_with_spaces() {
        let exe = r"C:\Program Files\Tolon\Tolon.exe";
        assert_eq!(
            windows_run_value(exe),
            r#""C:\Program Files\Tolon\Tolon.exe""#
        );
    }

    #[test]
    fn ac9_4_windows_run_value_is_compared_by_path() {
        let exe = r"C:\Program Files\Tolon\Tolon.exe";
        assert!(windows_run_value_is(&windows_run_value(exe), exe));
        assert!(windows_run_value_is(
            r#""c:\program files\tolon\TOLON.exe" --hidden"#,
            exe
        ));
        assert!(windows_run_value_is(
            r"C:\Program Files\Tolon\Tolon.exe",
            exe
        ));
        assert!(!windows_run_value_is(r#""C:\Old\Tolon.exe""#, exe));
        assert!(!windows_run_value_is(r"C:\Program", exe));
        assert!(!windows_run_value_is("", exe));
    }

    #[test]
    fn ac9_4_linux_exec_leaves_a_plain_path_alone() {
        let e = desktop_entry("/opt/tolon/tolon").unwrap_or_default();
        assert!(e.contains("\nExec=/opt/tolon/tolon\n"));
        assert!(e.contains("\nX-GNOME-Autostart-enabled=true\n"));
        assert!(e.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn ac9_4_linux_exec_quotes_paths_with_spaces_and_escapes_the_rest() {
        let exec = |p: &str| exec_line(p);
        assert_eq!(exec("/opt/My Apps/tolon"), r#""/opt/My Apps/tolon""#);
        assert_eq!(exec("/opt/100%/tolon"), "/opt/100%%/tolon");
        assert_eq!(exec(r#"/a b/"q"/$x/`y`"#), r#""/a b/\\"q\\"/\\$x/\\`y\\`""#);
        assert_eq!(exec(r"/a\b"), r#""/a\\\\b""#);
        assert_eq!(desktop_entry("/a\nb"), None);
    }

    #[test]
    fn ac9_4_linux_entry_reads_back_only_for_the_same_exe() {
        let exe = "/opt/My Apps/tolon";
        let entry = desktop_entry(exe).unwrap_or_default();
        assert!(desktop_entry_enables(&entry, exe));
        assert!(!desktop_entry_enables(&entry, "/opt/other/tolon"));
        let off = entry.replace("Autostart-enabled=true", "Autostart-enabled=false");
        assert!(!desktop_entry_enables(&off, exe));
        assert!(!desktop_entry_enables(
            &format!("{entry}Hidden=true\n"),
            exe
        ));
        assert!(!desktop_entry_enables("", exe));
    }

    #[test]
    fn ac9_4_linux_autostart_path_follows_the_xdg_rules() {
        let file = |x, h| autostart_file(x, h);
        let tail = Path::new("autostart").join("dev.tolon.desktop");
        assert_eq!(
            file(Some("/cfg"), Some("/home/a")),
            Some(Path::new("/cfg").join(&tail))
        );
        assert_eq!(
            file(None, Some("/home/a")),
            Some(Path::new("/home/a/.config").join(&tail))
        );
        assert_eq!(
            file(Some(""), Some("/home/a")),
            Some(Path::new("/home/a/.config").join(&tail))
        );
        assert_eq!(
            file(Some("rel"), Some("/home/a")),
            Some(Path::new("/home/a/.config").join(&tail))
        );
        assert_eq!(file(None, None), None);
        assert_eq!(file(None, Some("")), None);
    }

    #[test]
    fn ac9_4_linux_file_is_created_read_back_and_removed() -> Result<(), PlatformError> {
        let home = temp_home("roundtrip");
        let exe = "/opt/My Apps/tolon";
        let file = autostart_file(home.to_str(), None).ok_or(PlatformError::Unsupported)?;
        assert_eq!(read_autostart(&file, exe)?, StartAtLogin::Disabled);
        write_autostart(&file, exe)?;
        assert!(file.starts_with(home.join("autostart")));
        assert_eq!(read_autostart(&file, exe)?, StartAtLogin::Enabled);
        // A moved install: the file names another program, so this one does not start.
        assert_eq!(
            read_autostart(&file, "/opt/new/tolon")?,
            StartAtLogin::Disabled
        );
        write_autostart(&file, "/opt/new/tolon")?;
        assert_eq!(
            read_autostart(&file, "/opt/new/tolon")?,
            StartAtLogin::Enabled
        );
        remove_autostart(&file)?;
        assert_eq!(read_autostart(&file, exe)?, StartAtLogin::Disabled);
        remove_autostart(&file)?;
        let _ = std::fs::remove_dir_all(&home);
        Ok(())
    }
}
