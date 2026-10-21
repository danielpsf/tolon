//! Secrets in the OS keychain: macOS Keychain, Windows Credential Manager, the Secret Service on
//! Linux (`keyring`). Google refresh tokens live here and never in SQLite (AC13.3). CI and tests
//! use [`MemorySecrets`].

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// The keychain service every entry is filed under; the account is the calendar id.
pub const SERVICE: &str = "dev.tolon";

/// The keychain could not be read or written. Carries nothing: keyring errors can quote the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the system keychain is not available")]
pub struct SecretError;

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    /// Deleting an entry that is not there is not an error.
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

/// The OS keychain.
pub struct Keychain;

fn failed(op: &'static str) -> SecretError {
    tracing::warn!(op, "keychain call failed");
    SecretError
}

impl Secrets for Keychain {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(|_| failed("open"))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(failed("get")),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        keyring::Entry::new(SERVICE, key)
            .and_then(|e| e.set_password(value))
            .map_err(|_| failed("set"))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(|_| failed("open"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(failed("delete")),
        }
    }
}

/// An in-memory keychain for CI and tests.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl MemorySecrets {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Secrets for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        Ok(self.map().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.map().insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.map().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac13_3_memory_secrets_set_get_replace_delete() -> Result<(), SecretError> {
        let s = MemorySecrets::default();
        assert_eq!(s.get("cal-1")?, None);
        s.set("cal-1", "refresh-1")?;
        s.set("cal-2", "refresh-2")?;
        assert_eq!(s.get("cal-1")?.as_deref(), Some("refresh-1"));
        s.set("cal-1", "refresh-1b")?;
        assert_eq!(s.get("cal-1")?.as_deref(), Some("refresh-1b"));
        s.delete("cal-1")?;
        s.delete("cal-1")?;
        assert_eq!(s.get("cal-1")?, None);
        assert_eq!(s.get("cal-2")?.as_deref(), Some("refresh-2"));
        Ok(())
    }

    #[test]
    fn ac13_3_secret_error_says_nothing_about_the_secret() {
        assert_eq!(
            SecretError.to_string(),
            "the system keychain is not available"
        );
        assert_eq!(format!("{SecretError:?}"), "SecretError");
    }
}
