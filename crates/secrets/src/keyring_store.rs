//! The [`SecretStore`] backed by the OS keychain.

use keyring::Entry;

use crate::{SecretError, SecretKey, SecretStore, SecretValue, check_value, checked_stored_value};

/// Stores secrets in the OS keychain: Keychain Services on macOS, Credential Manager on
/// Windows, and the freedesktop Secret Service (GNOME Keyring, KWallet) on Linux.
///
/// The store is a zero-sized handle — every call goes to the platform keychain under the
/// service [`SecretKey::SERVICE`] and the account [`SecretKey::account`] renders.
#[derive(Default)]
pub struct KeyringStore;

impl KeyringStore {
    /// The OS keychain store.
    pub fn new() -> Self {
        Self
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        let entry = entry_for(key)?;
        let text = entry
            .get_password()
            .map_err(|error| map_error(key, error))?;
        checked_stored_value(key, SecretValue::new(text))
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        check_value(key, value)?;
        let entry = entry_for(key)?;
        entry
            .set_password(value.expose())
            .map_err(|error| map_error(key, error))
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        let entry = entry_for(key)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // A secret that is already gone is not a failure.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(key, error)),
        }
    }
}

fn entry_for(key: &SecretKey) -> Result<Entry, SecretError> {
    Entry::new(SecretKey::SERVICE, &key.account()).map_err(|error| map_error(key, error))
}

/// Translate a keychain failure into a [`SecretError`].
///
/// The keychain error enum attaches the stored bytes on two of its variants — the secret
/// itself — so the error is never forwarded, formatted or stored; those two variants are
/// matched first and dropped. The text that does reach [`SecretError`] is platform
/// diagnostics (attribute names, length limits, connection failures), which never carry a
/// secret value.
fn map_error(key: &SecretKey, error: keyring::Error) -> SecretError {
    match error {
        keyring::Error::NoEntry => SecretError::NotFound { key: key.clone() },
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(_, _) => {
            SecretError::UnusableSecret { key: key.clone() }
        }
        keyring::Error::Invalid(attribute, reason) => SecretError::InvalidKey {
            reason: format!("the keychain rejected '{attribute}': {reason}"),
        },
        keyring::Error::TooLong(attribute, limit) => SecretError::InvalidKey {
            reason: format!("the keychain limit for '{attribute}' is {limit} characters"),
        },
        keyring::Error::Ambiguous(_) => SecretError::BackendUnavailable {
            reason: "more than one keychain entry matches this key".to_string(),
        },
        error @ (keyring::Error::PlatformFailure(_)
        | keyring::Error::NoStorageAccess(_)
        | keyring::Error::BadStoreFormat(_)
        | keyring::Error::NoDefaultStore
        | keyring::Error::NotSupportedByStore(_)) => SecretError::BackendUnavailable {
            reason: error.to_string(),
        },
        _ => SecretError::BackendUnavailable {
            reason: "the OS keychain reported an unexpected failure".to_string(),
        },
    }
}

#[cfg(test)]
#[path = "keyring_store_tests.rs"]
mod tests;
