use super::KeyringStore;
use crate::{SecretError, SecretKey, SecretStore, SecretValue};

/// Deletes the fixture entry however the test ends: an early `?` (a locked keychain
/// failing `get`) or a panic between `set` and `delete`.
struct Cleanup<'a> {
    store: &'a KeyringStore,
    key: &'a SecretKey,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        // Best effort only: the entry is an obviously fake fixture and this runs while the
        // test is already ending, so there is no caller left to propagate a cleanup
        // failure to; matching the result is the deliberate decision to drop it.
        match self.store.delete(self.key) {
            Ok(()) => {}
            Err(_cleanup_failure) => {}
        }
    }
}

/// Round-trips one secret through the real OS keychain.
///
/// Not part of the default run: it needs a logged-in desktop keychain (macOS Keychain,
/// Windows Credential Manager, or a Secret Service on Linux). The human sign-off for S25
/// runs it with `cargo test -p secrets -- --ignored`. The value is an obviously fake
/// fixture, deleted before the assertions run on the happy path; a `Cleanup` guard
/// deletes it on any other exit (an early `?`, a panic), so every exit path attempts to
/// leave the real keychain as it found it.
#[test]
#[ignore]
fn live_keyring_roundtrip() -> Result<(), SecretError> {
    let key = SecretKey::new("pustman-self-test", "secrets", "live_keyring_roundtrip")?;
    let store = KeyringStore::new();
    let _cleanup = Cleanup {
        store: &store,
        key: &key,
    };

    store.set(&key, &SecretValue::new("token-REDACTED-example"))?;
    let stored = store.get(&key)?;
    store.delete(&key)?;
    let after_delete = store.get(&key);

    assert_eq!(
        stored.expose(),
        "token-REDACTED-example",
        "the keychain must hand back what it stored"
    );
    assert!(
        matches!(after_delete, Err(SecretError::NotFound { .. })),
        "a deleted keychain entry must report NotFound"
    );
    Ok(())
}
