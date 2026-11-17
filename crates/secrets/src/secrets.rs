//! Secret values in the OS keychain, behind a store trait.
//!
//! Pustman never writes a secret value to a workspace file, to SQLite or to a log. The
//! workspace records only a variable's key, kind and description (PRD REQ-VAR-SHARE-02),
//! Postman `secret`-typed values are imported straight into the keychain (REQ-IO-02), and
//! this crate is the only place that talks to the keychain.
//!
//! # Key layout
//!
//! A [`SecretKey`] names one secret by workspace, variable scope and variable key. Secrets
//! live under the keychain service [`SecretKey::SERVICE`] with the account rendering
//! `workspace_id/scope_id/variable_key`, so every secret of every workspace and scope stays
//! distinguishable under one service.
//!
//! # Invariants (proved by this crate's tests)
//!
//! - A secret *value* never appears in `Debug` or `Display` output. [`SecretValue`] prints
//!   `<redacted>` both ways and stores print `<redacted>` too; nothing here prints, logs or
//!   traces a value.
//! - [`SecretError`] carries keys and platform diagnostics, never values. Keychain errors
//!   that carry stored bytes are dropped before they reach a caller.
//! - [`SecretStore::get`] returns a non-empty value or an error ([`SecretError::NotFound`]
//!   for a secret that is not set). Empty values are refused on write and on read
//!   ([`SecretError::InvalidValue`]) — including a stored entry written outside this
//!   crate, edited in the keychain by hand or by another tool — so a secret that resolves
//!   to an empty string is always "unresolved", never a silently empty string on the wire.
//!
//! # Stores
//!
//! [`KeyringStore`] is the real one: Keychain Services on macOS, Credential Manager on
//! Windows, the freedesktop Secret Service (GNOME Keyring, KWallet) on Linux.
//! `MemorySecretStore` is an in-memory store for tests: compiled for this crate's own
//! tests and exported to other crates' tests behind the `test-support` feature.

mod keyring_store;
#[cfg(any(test, feature = "test-support"))]
mod memory_store;
mod secret_key;
mod secret_value;

pub use keyring_store::KeyringStore;
#[cfg(any(test, feature = "test-support"))]
pub use memory_store::MemorySecretStore;
pub use secret_key::SecretKey;
pub use secret_value::SecretValue;

/// Reads and writes secret values.
///
/// [`KeyringStore`] and `MemorySecretStore` agree on semantics: they answer the same
/// calls with the same results, so behavior proved against the memory store holds against
/// the OS keychain.
pub trait SecretStore: Send + Sync {
    /// The value stored under `key`.
    ///
    /// # Errors
    ///
    /// [`SecretError::NotFound`] when no secret is set for `key` (never set, or deleted);
    /// [`SecretError::InvalidValue`] when the stored entry is empty — an entry written
    /// outside [`SecretStore::set`] is refused just like a value `set` would refuse;
    /// [`SecretError::UnusableSecret`] when the stored entry cannot be read as text;
    /// [`SecretError::BackendUnavailable`] when the keychain cannot be reached.
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError>;

    /// Store `value` under `key`, replacing whatever was there.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidValue`] when `value` is empty: an empty secret is
    /// indistinguishable from an unset one, so it is refused here rather than sent as an
    /// empty string later.
    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError>;

    /// Remove the secret stored under `key`.
    ///
    /// Deleting a key that has no secret succeeds — callers use [`SecretStore::get`] to
    /// find out whether a secret exists.
    ///
    /// # Errors
    ///
    /// [`SecretError::BackendUnavailable`] when the keychain cannot be reached.
    fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
}

/// Why a secret operation failed.
///
/// Every variant names the key it concerns or a platform diagnostic. No variant holds a
/// secret value.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No secret is stored for this key: never set, or deleted. Callers surface this as
    /// "unresolved secret" (PRD REQ-VAR-SHARE-02) instead of sending an empty string.
    #[error("no secret is stored for {key}")]
    NotFound { key: SecretKey },

    /// The key cannot be represented in the OS keychain: an empty or NUL-carrying
    /// component, a `/` in an id component, or a keychain that refuses the entry name.
    #[error("secret key is not valid for the OS keychain: {reason}")]
    InvalidKey { reason: String },

    /// The value is empty: refused before it reached the keychain on
    /// [`SecretStore::set`], or found empty in a stored entry on [`SecretStore::get`].
    #[error("the secret value for {key} is empty; a secret must have a value")]
    InvalidValue { key: SecretKey },

    /// The OS keychain could not be reached or refused the operation.
    #[error("the OS keychain is unavailable: {reason}")]
    BackendUnavailable { reason: String },

    /// The keychain holds an entry Pustman cannot read as text; the secret must be set
    /// again.
    #[error("the stored secret for {key} cannot be read; set it again")]
    UnusableSecret { key: SecretKey },
}

/// Refuse values no secret store may hold, so every [`SecretStore`] accepts the same
/// values and [`SecretStore::get`] can never hand back an empty string.
fn check_value(key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
    if value.expose().is_empty() {
        return Err(SecretError::InvalidValue { key: key.clone() });
    }
    Ok(())
}

/// The value read back from a store, or [`SecretError::InvalidValue`] when it is empty.
///
/// [`check_value`] refuses to write an empty value, but a stored entry can also appear
/// outside [`SecretStore::set`] — edited by hand in Keychain Access or Credential
/// Manager, or written by another tool. Both stores run this over what they read, so both
/// answer [`SecretStore::get`] the same way and `get` never hands back an empty string.
fn checked_stored_value(key: &SecretKey, value: SecretValue) -> Result<SecretValue, SecretError> {
    check_value(key, &value)?;
    Ok(value)
}
