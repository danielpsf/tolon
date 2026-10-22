//! Logging (L-072): stderr as before, plus daily log files in the data folder for the diagnostics
//! zip. The files keep Tolon's own debug lines (never a dependency's: no `tracing-log`, L-061), at
//! most 7 days and 10 MB in all. Rule 4 holds in both: no URL with a secret, no token, no title,
//! and debug lines carry ids and codes only.
//!
//! The subscriber starts in `main`, before Tauri knows the data folder; the file layer writes
//! nowhere until [`start_files`] opens the folder in `setup`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use tracing::level_filters::LevelFilter;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// The folder under the data dir.
pub const FOLDER: &str = "logs";
const PREFIX: &str = "tolon";
const SUFFIX: &str = "log";
/// Days of files kept, today included.
pub const KEEP_DAYS: i64 = 7;
/// All files together; a line that would pass it is dropped until old files can go.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Tolon's own crates: debug in the files. Everything else stays at info.
const OWN_CRATES: [&str; 5] = [
    "tolon_app",
    "tolon_core",
    "tolon_store",
    "tolon_sync",
    "tolon_platform",
];

/// Info and below, unless `TOLON_LOG=debug` asks for the debug lines (on-screen checks read them,
/// L-061). Anything else is info. Stderr only; the files always keep Tolon's debug lines.
pub fn stderr_level(asked: Option<&str>) -> tracing::Level {
    match asked {
        Some("debug") => tracing::Level::DEBUG,
        _ => tracing::Level::INFO,
    }
}

/// Debug for Tolon's crates, info for every dependency.
pub fn file_filter() -> Targets {
    OWN_CRATES
        .iter()
        .fold(Targets::new().with_default(LevelFilter::INFO), |t, c| {
            t.with_target(*c, LevelFilter::DEBUG)
        })
}

/// The file layer: plain text, UTC timestamps with `Z`, module targets, [`file_filter`].
pub fn file_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(file_filter())
}

static FILES: OnceLock<LogFiles> = OnceLock::new();

/// Writes to the files once [`start_files`] opened them; nothing before.
struct Late;

impl Write for Late {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match FILES.get() {
            Some(mut files) => files.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match FILES.get() {
            Some(mut files) => files.flush(),
            None => Ok(()),
        }
    }
}

/// Installs the global subscriber: stderr at `stderr_level`, and the files (once started).
pub fn init(asked: Option<&str>) {
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_filter(LevelFilter::from_level(stderr_level(asked)));
    let _ = tracing_subscriber::registry()
        .with(stderr)
        .with(file_layer(|| Late))
        .try_init();
}

/// Opens `<data dir>/logs` for the global subscriber. A failure leaves stderr only.
pub fn start_files(data_dir: &Path) {
    match LogFiles::open(&data_dir.join(FOLDER)) {
        Ok(files) => {
            let _ = FILES.set(files);
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not open the log files; logging to stderr only")
        }
    }
}

/// The folder the running Tolon logs to, if the files are open.
pub fn folder() -> Option<PathBuf> {
    FILES.get().map(|f| f.dir.clone())
}

/// The daily files and the 10 MB budget. Each log line arrives as one `write` from the fmt layer.
pub struct LogFiles {
    dir: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    appender: RollingFileAppender,
    /// Bytes in the folder's log files, as far as Tolon knows.
    used: u64,
}

impl LogFiles {
    /// Creates the folder, drops files older than 7 days and over the budget, and opens today's.
    pub fn open(dir: &Path) -> io::Result<LogFiles> {
        fs::create_dir_all(dir)?;
        let used = prune(dir, Utc::now(), 0)?;
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(PREFIX)
            .filename_suffix(SUFFIX)
            .max_log_files(usize::try_from(KEEP_DAYS).unwrap_or(7))
            .build(dir)
            .map_err(io::Error::other)?;
        Ok(LogFiles {
            dir: dir.to_owned(),
            inner: Mutex::new(Inner { appender, used }),
        })
    }
}

impl Write for &LogFiles {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let len = buf.len() as u64;
        if inner.used + len > MAX_BYTES {
            // Rare: old files go first; today's file alone over the budget drops the line.
            inner.used = prune(&self.dir, Utc::now(), len).unwrap_or(inner.used);
            if inner.used + len > MAX_BYTES {
                return Ok(buf.len());
            }
        }
        inner.appender.write_all(buf)?;
        inner.used += len;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .appender
            .flush()
    }
}

/// The day of a `tolon.YYYY-MM-DD.log` name; `None` for any other file.
fn day_of(name: &str) -> Option<NaiveDate> {
    let day = name.strip_prefix("tolon.")?.strip_suffix(".log")?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

/// The log files in `dir`, oldest first, with their sizes.
pub fn list(dir: &Path) -> io::Result<Vec<(PathBuf, NaiveDate, u64)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(day) = name.to_str().and_then(day_of) else {
            continue;
        };
        files.push((entry.path(), day, entry.metadata()?.len()));
    }
    files.sort_by_key(|f| f.1);
    Ok(files)
}

/// Deletes files older than [`KEEP_DAYS`], then the oldest ones other than today's until `room`
/// more bytes fit in [`MAX_BYTES`]. Returns the bytes left. The files are dated in UTC, as
/// tracing-appender names them.
fn prune(dir: &Path, now: DateTime<Utc>, room: u64) -> io::Result<u64> {
    let today = now.date_naive();
    let oldest = today - Duration::days(KEEP_DAYS - 1);
    let mut kept = Vec::new();
    for (path, day, size) in list(dir)? {
        if day < oldest {
            fs::remove_file(&path)?;
        } else {
            kept.push((path, day, size));
        }
    }
    let mut used: u64 = kept.iter().map(|f| f.2).sum();
    for (path, day, size) in kept {
        if used + room <= MAX_BYTES || day >= today {
            break;
        }
        fs::remove_file(&path)?;
        used -= size;
    }
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    type TestResult = anyhow::Result<()>;

    #[test]
    fn l061_debug_lines_on_stderr_are_opt_in() {
        assert_eq!(stderr_level(None), tracing::Level::INFO);
        assert_eq!(stderr_level(Some("warn")), tracing::Level::INFO);
        assert_eq!(stderr_level(Some("debug")), tracing::Level::DEBUG);
    }

    #[test]
    fn l072_the_files_keep_tolons_debug_lines_and_only_info_from_dependencies() {
        let f = file_filter();
        assert!(f.would_enable("tolon_app::scheduler", &tracing::Level::DEBUG));
        assert!(f.would_enable("tolon_platform::macos", &tracing::Level::DEBUG));
        assert!(!f.would_enable("tolon_app", &tracing::Level::TRACE));
        assert!(!f.would_enable("reqwest::connect", &tracing::Level::DEBUG));
        assert!(!f.would_enable("tao::platform", &tracing::Level::DEBUG));
        assert!(f.would_enable("tauri::app", &tracing::Level::INFO));
    }

    fn at(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0)
            .single()
            .unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn l072_prune_keeps_seven_days_and_ten_megabytes() -> TestResult {
        let dir = tempfile::tempdir()?;
        let write =
            |name: &str, size: usize| std::fs::write(dir.path().join(name), vec![b'x'; size]);
        write("tolon.2026-10-14.log", 10)?; // 8 days before the 22nd: goes
        write("tolon.2026-10-16.log", 6 * 1024 * 1024)?; // oldest kept day, over the budget: goes
        write("tolon.2026-10-20.log", 3 * 1024 * 1024)?;
        write("tolon.2026-10-22.log", 2 * 1024 * 1024)?;
        write("notes.txt", 10)?;
        let used = prune(dir.path(), at(2026, 10, 22), 0)?;
        let left: Vec<String> = list(dir.path())?
            .iter()
            .filter_map(|f| f.0.file_name()?.to_str().map(str::to_owned))
            .collect();
        assert_eq!(left, ["tolon.2026-10-20.log", "tolon.2026-10-22.log"]);
        assert_eq!(used, 5 * 1024 * 1024);
        assert!(dir.path().join("notes.txt").is_file(), "only log files go");
        // Today's file is never deleted, even alone over the budget.
        assert_eq!(
            prune(dir.path(), at(2026, 10, 22), MAX_BYTES)?,
            2 * 1024 * 1024
        );
        assert!(dir.path().join("tolon.2026-10-22.log").is_file());
        Ok(())
    }

    #[test]
    fn l072_a_line_over_the_budget_is_dropped() -> TestResult {
        let dir = tempfile::tempdir()?;
        let files = LogFiles::open(dir.path())?;
        let mut w = &files;
        w.write_all(b"first line\n")?;
        let big = vec![b'y'; usize::try_from(MAX_BYTES)?];
        w.write_all(&big)?;
        w.flush()?;
        let total: u64 = list(dir.path())?.iter().map(|f| f.2).sum();
        assert_eq!(total, 11, "the oversized line was dropped");
        Ok(())
    }
}
