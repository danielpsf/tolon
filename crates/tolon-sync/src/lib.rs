//! ICS fetcher, Google Calendar client and OAuth.

pub mod google;
mod ics;
#[cfg(test)]
mod test_log;

pub use ics::{normalize_url, redact_url, FetchConfig, FetchError, FetchOutcome, IcsFetcher};

/// Crate name, a placeholder export until the real API lands.
pub const NAME: &str = env!("CARGO_PKG_NAME");
