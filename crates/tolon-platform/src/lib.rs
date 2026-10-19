//! OS-specific behaviour behind the [`Platform`] trait: window levels, screen
//! enumeration and sound. Platform code lives only in this crate; CI uses [`StubPlatform`].

/// What the app needs from the operating system.
pub trait Platform {
    /// Short implementation name, for logs and diagnostics.
    fn name(&self) -> &'static str;
}

/// No-op implementation for CI and tests.
pub struct StubPlatform;

impl Platform for StubPlatform {
    fn name(&self) -> &'static str {
        "stub"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_platform_reports_its_name() {
        assert_eq!(StubPlatform.name(), "stub");
    }
}
