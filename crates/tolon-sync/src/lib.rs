//! ICS fetcher, Google Calendar client and OAuth.

mod ics;

pub use ics::{redact_url, FetchConfig, FetchError, FetchOutcome, IcsFetcher};

/// Crate name, a placeholder export until the real API lands.
pub const NAME: &str = env!("CARGO_PKG_NAME");
