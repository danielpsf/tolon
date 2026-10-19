//! Domain types and pure functions: events, occurrences, expansion, ignore rules,
//! the alarm state machine and join-link detection. No I/O, no async runtime.

/// Crate name, a placeholder export until the real API lands.
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_set() {
        assert_eq!(NAME, "tolon-core");
    }
}
