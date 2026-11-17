//! An in-memory [`SecretStore`] for tests.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

use crate::{SecretError, SecretKey, SecretStore, SecretValue, check_value, checked_stored_value};

/// A [`SecretStore`] that keeps secrets in process memory.
///
/// This is a test double, not a fallback: it is compiled for this crate's own tests and
/// exported to other crates' tests behind the `test-support` feature. It implements the
/// same contract as [`crate::KeyringStore`], so a test written against it holds for the
/// OS keychain.
#[derive(Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, SecretValue>>,
}

impl MemorySecretStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, SecretValue>> {
        // A poisoned lock only means some thread panicked while holding it; the map is a
        // plain `HashMap` with no invariant to break, so take the guard back rather than
        // failing every later call.
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl fmt::Debug for MemorySecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MemorySecretStore(<redacted>)")
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        let value = self
            .entries()
            .get(&key.account())
            .cloned()
            .ok_or_else(|| SecretError::NotFound { key: key.clone() })?;
        checked_stored_value(key, value)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        check_value(key, value)?;
        self.entries().insert(key.account(), value.clone());
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.entries().remove(&key.account());
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_store_tests.rs"]
mod tests;
