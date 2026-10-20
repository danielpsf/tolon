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

/// The one `SMAppService` call a request needs, given what the OS reports now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmCall {
    /// The requested state already holds: succeed without calling.
    Nothing,
    Register,
    Unregister,
}

/// Which call turns `current` into the wanted state. Registered but waiting for approval counts
/// as registered: registering again cannot change it, and the UI already explains the wait.
pub fn sm_call_for(current: StartAtLogin, enabled: bool) -> SmCall {
    match (enabled, current) {
        (true, StartAtLogin::Disabled) => SmCall::Register,
        (true, _) | (false, StartAtLogin::Disabled) => SmCall::Nothing,
        (false, _) => SmCall::Unregister,
    }
}

/// Whether an `SMAppService` error means the wanted state holds anyway: on register 11 (launch
/// denied by the user, so approval is pending) and 12 (already registered); on unregister 6
/// (job not found, so nothing is registered).
pub fn sm_error_means_done(call: SmCall, code: isize) -> bool {
    match call {
        SmCall::Register => matches!(code, 11 | 12),
        SmCall::Unregister => code == 6,
        SmCall::Nothing => true,
    }
}

/// The `Run` value for the current exe: the path in double quotes, so a path with spaces
/// (`C:\Program Files\Tolon\Tolon.exe`) is not split at the first space.
pub fn windows_run_value(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Whether a `Run` value starts `exe`. Windows paths ignore case, including non-ASCII letters.
/// An unquoted value, or a quoted one followed by arguments, still counts when its program is
/// `exe`.
pub fn windows_run_value_is(value: &str, exe: &str) -> bool {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => value,
    };
    program.to_lowercase() == exe.to_lowercase()
}

/// Whether the `StartupApproved\Run` data of a value (Task Manager, Startup apps) says the
/// person turned it off: the low bit of the first byte is set (0x02 and 0x06 enabled, 0x03
/// disabled). Missing or empty data is not a refusal. Tolon reads this and never writes it.
pub fn startup_approved_disabled(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 1 == 1)
}

/// The program to autostart on Linux: `$APPIMAGE` when it is an absolute path (inside an
/// AppImage the running exe is a temporary mount), else `None` and the caller uses its own exe.
pub fn appimage_program(appimage: Option<&str>) -> Option<&str> {
    // A string test, not `Path::is_absolute`, so the same answer comes out on every OS.
    appimage.filter(|p| p.starts_with('/'))
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
/// `Hidden=true` nor `X-GNOME-Autostart-enabled=false`. Only the `[Desktop Entry]` group counts;
/// the same keys in another group, such as a `[Desktop Action ...]`, say nothing about login.
pub fn desktop_entry_enables(contents: &str, exe: &str) -> bool {
    let mut in_entry = false;
    let mut keys: Vec<(&str, &str)> = Vec::new();
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some((k, v)) = line.split_once('=') {
                keys.push((k.trim(), v.trim()));
            }
        }
    }
    let value = |key: &str| keys.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    value("Exec") == Some(exec_line(exe).as_str())
        && value("Hidden") != Some("true")
        && value("X-GNOME-Autostart-enabled") != Some("false")
}

/// `<config home>/autostart/dev.tolon.desktop`. The config home is `XDG_CONFIG_HOME` when it is
/// an absolute path (the spec ignores a relative one), else `$HOME/.config`.
pub fn autostart_file(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    // A string test, not `Path::is_absolute`, so the same answer comes out on every OS.
    let config = match xdg_config_home {
        Some(p) if p.starts_with('/') => PathBuf::from(p),
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

/// Create missing directories, owner-only on Unix as the XDG base directory spec asks.
fn create_dirs(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Create or replace the autostart file at `file` so it starts `exe`. The entry goes to a new
/// temporary file in the same directory (`create_new` never follows a link or reuses a file) and
/// is renamed over `file`, so a symlink at `file` is replaced, not written through.
pub fn write_autostart(file: &Path, exe: &str) -> Result<(), PlatformError> {
    use std::io::Write;
    let entry = desktop_entry(exe)
        .ok_or_else(|| PlatformError::Io("the program path holds a line break".into()))?;
    let dir = file
        .parent()
        .ok_or_else(|| PlatformError::Io("the autostart path has no directory".into()))?;
    create_dirs(dir).map_err(io)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let tmp = dir.join(format!(
        ".dev.tolon.desktop.{}.{nanos}.tmp",
        std::process::id()
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(entry.as_bytes()))
        .and_then(|()| std::fs::rename(&tmp, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map_err(io)
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
        let file = home.join("autostart").join("dev.tolon.desktop");
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

    #[test]
    fn ac9_4_macos_set_is_idempotent() {
        use StartAtLogin::{Disabled, Enabled, RequiresApproval};
        assert_eq!(sm_call_for(Disabled, true), SmCall::Register);
        assert_eq!(sm_call_for(Enabled, true), SmCall::Nothing);
        assert_eq!(sm_call_for(RequiresApproval, true), SmCall::Nothing);
        assert_eq!(sm_call_for(Disabled, false), SmCall::Nothing);
        assert_eq!(sm_call_for(Enabled, false), SmCall::Unregister);
        assert_eq!(sm_call_for(RequiresApproval, false), SmCall::Unregister);
    }

    #[test]
    fn ac9_4_macos_errors_that_mean_the_state_already_holds() {
        assert!(sm_error_means_done(SmCall::Register, 11));
        assert!(sm_error_means_done(SmCall::Register, 12));
        assert!(!sm_error_means_done(SmCall::Register, 6));
        assert!(!sm_error_means_done(SmCall::Register, 1));
        assert!(sm_error_means_done(SmCall::Unregister, 6));
        assert!(!sm_error_means_done(SmCall::Unregister, 11));
        assert!(!sm_error_means_done(SmCall::Unregister, 12));
    }

    #[test]
    fn ac9_4_windows_paths_compare_with_unicode_case_folding() {
        assert!(windows_run_value_is(
            r#""C:\Users\JOSÉ\Tolon\Tolon.exe""#,
            r"c:\users\josé\tolon\tolon.exe"
        ));
        assert!(!windows_run_value_is(
            r#""C:\Users\José\a.exe""#,
            r"C:\Users\Jose\a.exe"
        ));
    }

    #[test]
    fn ac9_4_windows_task_manager_disable_is_read_from_the_first_byte() {
        assert!(!startup_approved_disabled(&[0x02, 0, 0, 0, 0, 0]));
        assert!(!startup_approved_disabled(&[0x06, 0, 0, 0]));
        assert!(startup_approved_disabled(&[0x03, 0xAA, 0, 0]));
        assert!(startup_approved_disabled(&[0x01]));
        assert!(!startup_approved_disabled(&[]));
    }

    #[test]
    fn ac9_4_linux_appimage_counts_only_as_an_absolute_path() {
        assert_eq!(
            appimage_program(Some("/tmp/Tolon.AppImage")),
            Some("/tmp/Tolon.AppImage")
        );
        assert_eq!(appimage_program(Some("Tolon.AppImage")), None);
        assert_eq!(appimage_program(Some("./Tolon.AppImage")), None);
        assert_eq!(appimage_program(Some("")), None);
        assert_eq!(appimage_program(None), None);
    }

    #[test]
    fn ac9_4_linux_only_the_desktop_entry_group_counts() {
        let exe = "/opt/tolon/tolon";
        let entry = desktop_entry(exe).unwrap_or_default();
        let other_group =
            "[Desktop Entry]\nType=Application\n\n[Desktop Action x]\nExec=/opt/tolon/tolon\n";
        assert!(!desktop_entry_enables(other_group, exe));
        let hidden_elsewhere = format!("{entry}\n[Desktop Action x]\nHidden=true\n");
        assert!(desktop_entry_enables(&hidden_elsewhere, exe));
        let hidden_in_entry = entry.replace("Terminal=false", "Hidden=true");
        assert!(!desktop_entry_enables(&hidden_in_entry, exe));
    }

    #[cfg(unix)]
    #[test]
    fn ac9_4_linux_a_symlinked_target_is_replaced_not_followed() -> Result<(), PlatformError> {
        let home = temp_home("symlink");
        let dir = home.join("autostart");
        std::fs::create_dir_all(&dir).map_err(io)?;
        let victim = home.join("victim.txt");
        std::fs::write(&victim, "keep me").map_err(io)?;
        let file = dir.join("dev.tolon.desktop");
        std::os::unix::fs::symlink(&victim, &file).map_err(io)?;
        write_autostart(&file, "/opt/tolon/tolon")?;
        assert_eq!(std::fs::read_to_string(&victim).map_err(io)?, "keep me");
        let meta = std::fs::symlink_metadata(&file).map_err(io)?;
        assert!(!meta.file_type().is_symlink());
        assert_eq!(
            read_autostart(&file, "/opt/tolon/tolon")?,
            StartAtLogin::Enabled
        );
        let entries = std::fs::read_dir(&dir).map_err(io)?.count();
        assert_eq!(entries, 1, "no temporary file stays behind");
        let _ = std::fs::remove_dir_all(&home);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn ac9_4_linux_new_directories_are_owner_only() -> Result<(), PlatformError> {
        use std::os::unix::fs::PermissionsExt;
        let home = temp_home("mode");
        let config = home.join("config");
        let file = config.join("autostart").join("dev.tolon.desktop");
        write_autostart(&file, "/opt/tolon/tolon")?;
        for dir in [config.clone(), config.join("autostart")] {
            let mode = std::fs::metadata(&dir).map_err(io)?.permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", dir.display());
        }
        let _ = std::fs::remove_dir_all(&home);
        Ok(())
    }
}
