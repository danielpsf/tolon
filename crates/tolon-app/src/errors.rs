//! Machine-readable errors. Rust never sends an English sentence to the UI: the UI turns these
//! codes into localised copy. No code carries a URL or any user text.

use std::fmt;

use serde::Serialize;
use tolon_store::StoreError;
use tolon_sync::FetchError;

/// A failure the UI can name. `code` is one of the constants below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppError(pub &'static str);

impl AppError {
    pub const UNSUPPORTED_SCHEME: Self = Self("unsupported_scheme");
    pub const INVALID_URL: Self = Self("invalid_url");
    pub const NOT_FOUND: Self = Self("not_found");
    pub const NO_URL: Self = Self("no_url");
    pub const NOT_READY: Self = Self("not_ready");
    pub const DB_ERROR: Self = Self("db_error");
    pub const INTERNAL: Self = Self("internal");
    /// The OS refused to add or remove the login item (AC9.4), such as macOS for a build that
    /// is not an installed app bundle.
    pub const LOGIN_ITEM: Self = Self("login_item");
    /// A check for updates the person asked for failed (offline, GitHub unreachable). The
    /// background check fails silently instead (AC11.1).
    pub const UPDATE_CHECK: Self = Self("update_check");
    /// The update did not download, verify or install; Tolon keeps running the old version.
    pub const UPDATE_INSTALL: Self = Self("update_install");
    /// This build has no Google client ID, so there is no Google sign-in.
    pub const GOOGLE_UNAVAILABLE: Self = Self("google_unavailable");
    /// The person refused access on Google's page.
    pub const GOOGLE_DENIED: Self = Self("google_denied");
    /// The sign-in was cancelled from Tolon, or another one replaced it.
    pub const GOOGLE_CANCELLED: Self = Self("google_cancelled");
    /// The sign-in did not finish: no browser, no answer in time, network or Google errors.
    pub const GOOGLE_SIGN_IN: Self = Self("google_sign_in");
    /// The system keychain could not store or read the sign-in.
    pub const KEYCHAIN: Self = Self("keychain");
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for AppError {}

/// What a failed command returns to JS: `{ "code": "invalid_url" }`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: &'static str,
}

/// Maps any pipeline error to its code. Unknown failures are `internal`; the cause is logged, never sent.
pub fn command_error(e: &anyhow::Error) -> CommandError {
    let code = if let Some(app) = e.downcast_ref::<AppError>() {
        app.0
    } else if let Some(store) = e.downcast_ref::<StoreError>() {
        match store {
            StoreError::NotFound => AppError::NOT_FOUND.0,
            _ => AppError::DB_ERROR.0,
        }
    } else {
        AppError::INTERNAL.0
    };
    if code == AppError::DB_ERROR.0 || code == AppError::INTERNAL.0 {
        tracing::warn!(code, error = %e, "command failed");
    }
    CommandError { code }
}

/// The stable code stored in `calendars.last_error` and `sync_log.detail` for a failed fetch.
pub fn fetch_code(e: &FetchError) -> String {
    match e {
        FetchError::Status(status) => format!("http:{status}"),
        FetchError::Timeout => "timeout".to_owned(),
        FetchError::Connect => "connect".to_owned(),
        FetchError::NotCalendar => "not_calendar".to_owned(),
        FetchError::TooLarge { .. } => "too_large".to_owned(),
        FetchError::Network => "network".to_owned(),
        FetchError::UnsupportedScheme => "unsupported_scheme".to_owned(),
        FetchError::InvalidUrl => "invalid_url".to_owned(),
    }
}

/// `last_error` as the UI gets it: `{ "kind": "http", "status": 404 }` or `{ "kind": "timeout" }`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SyncErrorDto {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl SyncErrorDto {
    /// Reads a stored code; `http:404` splits into kind and status, anything else is the kind.
    pub fn from_code(code: &str) -> Self {
        match code.split_once(':') {
            Some((kind, status)) => Self {
                kind: kind.to_owned(),
                status: status.parse().ok(),
            },
            None => Self {
                kind: code.to_owned(),
                status: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_into_the_dto() {
        assert_eq!(
            SyncErrorDto::from_code(&fetch_code(&FetchError::Status(404))),
            SyncErrorDto {
                kind: "http".into(),
                status: Some(404)
            }
        );
        assert_eq!(
            SyncErrorDto::from_code("timeout"),
            SyncErrorDto {
                kind: "timeout".into(),
                status: None
            }
        );
    }

    #[test]
    fn command_errors_carry_only_a_code() {
        assert_eq!(
            command_error(&anyhow::Error::new(AppError::INVALID_URL)).code,
            "invalid_url"
        );
        assert_eq!(
            command_error(&anyhow::Error::new(StoreError::NotFound)).code,
            "not_found"
        );
        assert_eq!(command_error(&anyhow::anyhow!("boom")).code, "internal");
    }
}
