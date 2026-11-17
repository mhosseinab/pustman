use super::MemorySecretStore;
use crate::{SecretError, SecretKey, SecretStore, SecretValue};

#[test]
fn memory_store_get_rejects_an_empty_stored_entry() -> Result<(), SecretError> {
    let key = SecretKey::new("workspace-1", "environment-2", "api_key")?;
    let store = MemorySecretStore::new();

    // An entry written outside `SecretStore::set`, which the real keychain can hold (the
    // user clears the value in Keychain Access or Credential Manager, or another tool
    // writes it): the same rejection `set` performs must also happen on read.
    store.entries().insert(key.account(), SecretValue::new(""));

    assert!(
        matches!(store.get(&key), Err(SecretError::InvalidValue { .. })),
        "an empty stored entry must be rejected on read, never handed back as an empty secret"
    );
    Ok(())
}

#[test]
fn memory_store_set_get_delete() -> Result<(), SecretError> {
    let key = SecretKey::new("workspace-1", "environment-2", "api_key")?;
    let store = MemorySecretStore::new();

    assert!(
        matches!(store.get(&key), Err(SecretError::NotFound { .. })),
        "a key with no secret must report NotFound"
    );

    store.set(&key, &SecretValue::new("token-REDACTED-example"))?;
    assert_eq!(store.get(&key)?.expose(), "token-REDACTED-example");

    assert!(
        matches!(
            store.set(&key, &SecretValue::new("")),
            Err(SecretError::InvalidValue { .. })
        ),
        "an empty value must be refused so a secret never resolves to an empty string"
    );

    store.delete(&key)?;
    assert!(
        matches!(store.get(&key), Err(SecretError::NotFound { .. })),
        "a deleted key must report NotFound"
    );
    assert!(
        matches!(store.delete(&key), Ok(())),
        "deleting a key with no secret must be a no-op"
    );
    Ok(())
}
