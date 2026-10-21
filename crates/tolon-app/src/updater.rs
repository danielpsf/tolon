//! F11 updates (AC11.1): check GitHub Releases on launch and then daily, never install without the
//! person's click, remember "Skip this version". The plugin fetches `latest.json` from the endpoint
//! in `tauri.conf.json` and verifies the minisign signature against the pubkey there before any
//! install; nothing here touches the bytes.
//!
//! The prompt is never a takeover: the settings window's About page holds the buttons, and the tray
//! panel shows one quiet line while an update waits. Both read [`UpdateDto`] (`update_status`, and
//! the `update:changed` event).
//!
//! Release builds only. A debug build (`cargo run`, the WebDriver build) never checks unless the
//! end-to-end command has set a loopback endpoint ([`UpdaterState::set_endpoint`]).

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use semver::Version;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::errors::AppError;
use crate::pipeline::{self, Db};

/// The version the person skipped (a semver string), in the settings table. App state, not a
/// setting: `SettingsDto` does not carry it and `settings_set` refuses it, like `paused_until`.
pub const SKIPPED: &str = "update_skipped";
/// A good check is followed by the next one a day later (AC11.1 "daily").
pub const EVERY: Duration = Duration::hours(24);
/// How often the loop looks at the clock. A failed check is retried at the next look, and the
/// wall clock is used, so a laptop that slept through the 24 hours checks soon after it wakes.
const LOOK_EVERY: StdDuration = StdDuration::from_secs(60 * 60);
/// The plugin's request timeout for `latest.json`; without one a hung server holds the check forever.
const CHECK_TIMEOUT: StdDuration = StdDuration::from_secs(30);
/// The plugin's request timeout for the package download (the plugin leaves `Update.timeout` unset).
const DOWNLOAD_TIMEOUT: StdDuration = StdDuration::from_secs(10 * 60);
/// Our own bound around each plugin call, a little past its request timeout, in case the plugin
/// waits outside the request.
const GRACE: StdDuration = StdDuration::from_secs(5);

/// What the About page and the panel show. `available` is the version to offer, `None` when there
/// is none, it was skipped, or the person chose Later; `installing` while the download runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UpdateDto {
    pub available: Option<String>,
    pub installing: bool,
}

/// Whether `available` is offered when `skipped` was skipped: only a version newer than the
/// skipped one prompts again. A skipped value that is not a version skips nothing.
pub fn prompts(available: &str, skipped: Option<&str>) -> bool {
    let Ok(available) = Version::parse(available) else {
        return false;
    };
    match skipped.and_then(|s| Version::parse(s).ok()) {
        Some(skipped) => available > skipped,
        None => true,
    }
}

/// Whether a check is due: never checked, a day since the last good one, or the clock went back.
pub fn due(last_good: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    last_good.is_none_or(|t| now < t || now - t >= EVERY)
}

/// Whether this build checks: a release build always, a debug build only with a test endpoint.
pub fn checks(release: bool, test_endpoint: bool) -> bool {
    release || test_endpoint
}

/// The prompt from what the last check found, the skipped version and Later.
pub fn status(
    found: Option<&str>,
    skipped: Option<&str>,
    later: bool,
    installing: bool,
) -> UpdateDto {
    let offered = found.filter(|v| installing || (!later && prompts(v, skipped)));
    UpdateDto {
        available: offered.map(str::to_owned),
        installing,
    }
}

#[derive(Default)]
struct Inner {
    /// The release the last good check found, newer than the running one.
    found: Option<Arc<Update>>,
    /// Later: no prompt until the next check finds something.
    later: bool,
    installing: bool,
    last_good: Option<DateTime<Utc>>,
    /// Set only by the end-to-end command (a loopback `latest.json`); `None` in every release.
    endpoint: Option<Url>,
}

/// The updater's memory, managed by the app. Only the skipped version is stored.
#[derive(Default)]
pub struct UpdaterState(Mutex<Inner>);

impl UpdaterState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The end-to-end build's endpoint (see `commands::update_e2e_endpoint`, its only caller, which
    /// only that build compiles; so other builds see it unused).
    #[allow(dead_code)]
    pub fn set_endpoint(&self, url: Url) {
        self.lock().endpoint = Some(url);
    }

    fn dto(&self, skipped: Option<&str>) -> UpdateDto {
        let inner = self.lock();
        let found = inner.found.as_ref().map(|u| u.version.as_str());
        status(found, skipped, inner.later, inner.installing)
    }
}

/// The skipped version, if any.
pub fn skipped(db: &Db) -> anyhow::Result<Option<String>> {
    Ok(pipeline::lock(db)
        .settings()
        .get(SKIPPED)?
        .and_then(|v| v.as_str().map(str::to_owned)))
}

fn updater(app: &AppHandle) -> anyhow::Result<tauri::State<'_, UpdaterState>> {
    app.try_state::<UpdaterState>()
        .ok_or_else(|| AppError::NOT_READY.into())
}

/// What `update_status` answers.
pub fn current(app: &AppHandle) -> anyhow::Result<UpdateDto> {
    let state = crate::commands::state(app)?;
    let skipped = skipped(&state.db)?;
    Ok(updater(app)?.dto(skipped.as_deref()))
}

/// Tells the settings window and the panel that the prompt changed.
fn announce(app: &AppHandle) {
    match current(app) {
        Ok(dto) => {
            if let Err(e) = app.emit("update:changed", dto) {
                tracing::warn!(error = %e, "could not emit update:changed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "could not read the update state"),
    }
}

/// A short reason for the debug log, never a URL: request errors keep only their kind and status.
fn reason(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(r) => format!(
            "request failed (status {:?}, timeout {}, connect {})",
            r.status().map(|s| s.as_u16()),
            r.is_timeout(),
            r.is_connect()
        ),
        E::Network(_) => "download failed".to_owned(),
        E::UrlParse(_) => "bad url".to_owned(),
        other => other.to_string(),
    }
}

/// One check. `Ok(false)` when this build does not check. A failure is the caller's to report:
/// the loop logs it at debug and retries later, `update_check_now` answers `update_check`.
pub async fn check(app: &AppHandle) -> anyhow::Result<bool> {
    let endpoint = updater(app)?.lock().endpoint.clone();
    if !checks(!cfg!(debug_assertions), endpoint.is_some()) {
        return Ok(false);
    }
    // The plugin's own hook only cleans up Tauri; on Windows `install` ends the process at once, so
    // the store is settled first, as on any exit (`settle_store` in main.rs).
    let handle = app.clone();
    let mut builder = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        .on_before_exit(move || {
            crate::settle_store(&handle);
            handle.cleanup_before_exit();
        });
    if let Some(url) = endpoint {
        builder = builder.endpoints(vec![url])?;
    }
    let plugin = builder.build()?;
    let found = bounded(CHECK_TIMEOUT + GRACE, plugin.check())
        .await
        .map_err(|reason| {
            tracing::debug!(%reason, "update check failed");
            AppError::UPDATE_CHECK
        })?;
    {
        let state = updater(app)?;
        let mut inner = state.lock();
        if !inner.installing {
            if let Some(update) = &found {
                tracing::info!(version = %update.version, "an update is available");
            }
            inner.found = found.map(Arc::new);
            inner.later = false;
        }
        inner.last_good = Some(Utc::now());
    }
    announce(app);
    Ok(true)
}

/// A plugin call with a hard limit; the error is a short reason for the debug log, never a URL.
async fn bounded<T>(
    limit: StdDuration,
    call: impl Future<Output = Result<T, tauri_plugin_updater::Error>>,
) -> Result<T, String> {
    match tokio::time::timeout(limit, call).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(reason(&e)),
        Err(_) => Err("timed out".to_owned()),
    }
}

/// What one look of the loop did.
#[derive(Debug, PartialEq, Eq)]
enum Look {
    /// Not due yet.
    Waited,
    Checked,
    /// Failed or timed out: `last_good` is unchanged, so the next look tries again.
    Failed,
    /// This build does not check: the loop ends.
    Off,
}

/// One look: `check` (answering like [`check`]) when due, bounded by `limit` as a whole.
async fn look<F: Future<Output = anyhow::Result<bool>>>(
    last_good: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    limit: StdDuration,
    check: impl FnOnce() -> F,
) -> Look {
    if !due(last_good, now) {
        return Look::Waited;
    }
    match tokio::time::timeout(limit, check()).await {
        Ok(Ok(true)) => Look::Checked,
        Ok(Ok(false)) => Look::Off,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "no update check this time");
            Look::Failed
        }
        Err(_) => {
            tracing::debug!("the update check timed out");
            Look::Failed
        }
    }
}

/// On launch, then daily, for the life of the app (AC11.1). Errors are silent and retried.
pub async fn run(app: AppHandle) {
    loop {
        let last_good = match updater(&app) {
            Ok(state) => state.lock().last_good,
            Err(_) => None,
        };
        let limit = CHECK_TIMEOUT + GRACE + GRACE;
        if look(last_good, Utc::now(), limit, || check(&app)).await == Look::Off {
            return;
        }
        tokio::time::sleep(LOOK_EVERY).await;
    }
}

/// "Later": no prompt until the next check.
pub fn later(app: &AppHandle) -> anyhow::Result<UpdateDto> {
    updater(app)?.lock().later = true;
    announce(app);
    current(app)
}

/// "Skip this version": stored, so the prompt stays away after a restart until a newer version.
pub fn skip(app: &AppHandle) -> anyhow::Result<UpdateDto> {
    let version = updater(app)?
        .lock()
        .found
        .as_ref()
        .map(|u| u.version.clone())
        .ok_or(AppError::NOT_FOUND)?;
    let state = crate::commands::state(app)?;
    pipeline::lock(&state.db)
        .settings()
        .set(SKIPPED, &json!(version))?;
    announce(app);
    current(app)
}

/// "Install and restart": downloads, verifies (inside the plugin; a bad signature installs
/// nothing), installs, stops the scheduler and restarts. On Windows the installer ends the process.
pub async fn install(app: &AppHandle) -> anyhow::Result<()> {
    let update = {
        let state = updater(app)?;
        let mut inner = state.lock();
        if inner.installing {
            return Ok(());
        }
        let update = inner.found.clone().ok_or(AppError::NOT_FOUND)?;
        inner.installing = true;
        update
    };
    announce(app);
    let mut update = (*update).clone();
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let download = update.download_and_install(|_, _| {}, || {});
    let done = finish_install(&*updater(app)?, DOWNLOAD_TIMEOUT + GRACE, download).await;
    if done.is_err() {
        announce(app);
    }
    done?;
    tracing::info!(version = %update.version, "update installed, restarting");
    if let (Some(scheduler), Ok(state)) = (
        app.try_state::<crate::commands::AppScheduler>(),
        crate::commands::state(app),
    ) {
        scheduler.shutdown(&state.db).await;
    }
    app.restart();
}

/// Waits for the download and install at most `limit`. A failure or a timeout ends `installing`, so
/// the About page offers the buttons again, and answers `update_install`.
async fn finish_install(
    state: &UpdaterState,
    limit: StdDuration,
    download: impl Future<Output = Result<(), tauri_plugin_updater::Error>>,
) -> anyhow::Result<()> {
    if let Err(reason) = bounded(limit, download).await {
        tracing::warn!(%reason, "the update did not install");
        state.lock().installing = false;
        return Err(AppError::UPDATE_INSTALL.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tolon_store::Store;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn ac11_1_a_skipped_version_does_not_prompt_and_a_newer_one_does() {
        assert!(prompts("0.2.0", None));
        assert!(!prompts("0.2.0", Some("0.2.0")));
        assert!(!prompts("0.1.9", Some("0.2.0")));
        assert!(prompts("0.2.1", Some("0.2.0")));
        assert!(prompts("0.2.0", Some("0.2.0-beta.3")));
        assert!(prompts("0.1.0-beta.2", Some("0.1.0-beta.1")));
        assert!(!prompts("0.1.0-beta.1", Some("0.1.0-beta.2")));
        // A skip that is not a version skips nothing; a found value that is not one never prompts.
        assert!(prompts("0.2.0", Some("garbage")));
        assert!(!prompts("not a version", None));
    }

    #[test]
    fn ac11_1_checks_on_launch_then_a_day_after_the_last_good_check() {
        let last = at("2026-10-21T08:00:00Z");
        assert!(due(None, last));
        assert!(!due(Some(last), at("2026-10-21T20:00:00Z")));
        assert!(!due(Some(last), at("2026-10-22T07:59:59Z")));
        assert!(due(Some(last), at("2026-10-22T08:00:00Z")));
        // A clock that went back does not hold the next check off for days.
        assert!(due(Some(last), at("2026-10-20T08:00:00Z")));
    }

    #[test]
    fn ac11_1_only_release_builds_check_unless_a_test_endpoint_is_set() {
        assert!(checks(true, false));
        assert!(checks(true, true));
        assert!(checks(false, true));
        assert!(!checks(false, false));
    }

    #[test]
    fn ac11_1_the_prompt_follows_the_found_version_skip_and_later() {
        let none = UpdateDto::default();
        assert_eq!(status(None, None, false, false), none);
        let offered = UpdateDto {
            available: Some("0.2.0".into()),
            installing: false,
        };
        assert_eq!(status(Some("0.2.0"), None, false, false), offered);
        assert_eq!(status(Some("0.2.0"), Some("0.2.0"), false, false), none);
        assert_eq!(status(Some("0.2.0"), None, true, false), none);
        assert_eq!(
            status(Some("0.3.0"), Some("0.2.0"), false, false).available,
            Some("0.3.0".into())
        );
        // While installing the version stays on screen whatever else changed.
        assert_eq!(
            status(Some("0.2.0"), Some("0.2.0"), true, true),
            UpdateDto {
                available: Some("0.2.0".into()),
                installing: true,
            }
        );
    }

    #[test]
    fn ac11_1_the_skipped_version_is_stored_outside_the_settings_contract() -> anyhow::Result<()> {
        let db: Db = std::sync::Mutex::new(Store::open_in_memory()?);
        assert_eq!(skipped(&db)?, None);
        pipeline::lock(&db)
            .settings()
            .set(SKIPPED, &json!("0.2.0"))?;
        assert_eq!(skipped(&db)?, Some("0.2.0".into()));
        // `settings_set` refuses the key: it is not part of the contract.
        let refused = crate::settings::apply(&db, &json!({ SKIPPED: "9.9.9" }));
        let code = refused.err().map(|e| crate::errors::command_error(&e).code);
        assert_eq!(code, Some("invalid:patch"));
        assert_eq!(skipped(&db)?, Some("0.2.0".into()));
        let dto = serde_json::to_value(crate::settings::get(&db)?)?;
        assert!(dto.get(SKIPPED).is_none());
        Ok(())
    }

    const SHORT: StdDuration = StdDuration::from_millis(50);

    #[tokio::test]
    async fn ac11_1_a_hung_check_gives_up_and_the_next_look_retries() {
        let now = at("2026-10-21T08:00:00Z");
        let hung = look(
            None,
            now,
            SHORT,
            std::future::pending::<anyhow::Result<bool>>,
        )
        .await;
        assert_eq!(hung, Look::Failed);
        // Nothing was recorded as a good check, so the next look (an hour later) is due and tries again.
        let next = now + Duration::hours(1);
        assert_eq!(
            look(None, next, SHORT, || async { Ok(true) }).await,
            Look::Checked
        );
        let offline = look(None, next, SHORT, || async {
            Err(anyhow::anyhow!("offline"))
        })
        .await;
        assert_eq!(offline, Look::Failed);
        assert_eq!(
            look(Some(now), next, SHORT, || async { Ok(true) }).await,
            Look::Waited
        );
        assert_eq!(
            look(None, next, SHORT, || async { Ok(false) }).await,
            Look::Off
        );
    }

    #[tokio::test]
    async fn ac11_1_a_hung_download_ends_installing_and_answers_update_install() {
        let state = UpdaterState::default();
        state.lock().installing = true;
        let done = finish_install(&state, SHORT, std::future::pending()).await;
        let code = done.err().map(|e| crate::errors::command_error(&e).code);
        assert_eq!(code, Some("update_install"));
        assert!(!state.lock().installing);
        assert_eq!(state.dto(None), UpdateDto::default());
        // A plugin error (a refused signature, a dropped connection) ends the same way.
        state.lock().installing = true;
        let refused = finish_install(&state, SHORT, async {
            Err(tauri_plugin_updater::Error::Network("refused".into()))
        })
        .await;
        assert!(refused.is_err());
        assert!(!state.lock().installing);
        assert!(finish_install(&state, SHORT, async { Ok(()) })
            .await
            .is_ok());
    }

    #[test]
    fn ac11_1_tauri_conf_sets_the_updater_for_github_releases() -> anyhow::Result<()> {
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], json!(true));
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"],
            json!(["https://github.com/danielpsf/tolon/releases/latest/download/latest.json"])
        );
        assert_eq!(updater["windows"]["installMode"], json!("passive"));
        // The signed version must match latest.json's, so a tampered manifest cannot roll back.
        assert_eq!(updater["requireSignedVersion"], json!(true));
        // The maintainer's key, id D3C5EBF2CE6A842B (decision "Releases use the Tauri CLI"); the
        // release preflight checks the signing key against it.
        assert_eq!(updater["pubkey"], json!(PUBKEY));
        Ok(())
    }

    const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEQzQzVFQkYyQ0U2QTg0MkIKUldRcmhHck84dXZGMCt4N1NpWUIzL20rQVRHWE5aNzJzMm1DMjRCaTNJdHVxdFdXOXo2K1Y5U1oK";
}
