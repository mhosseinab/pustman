use super::SecretValue;
use crate::{MemorySecretStore, SecretError, SecretKey, SecretStore};

#[test]
fn debug_output_hides_value() -> Result<(), SecretError> {
    let secret = SecretValue::new("token-REDACTED-example");

    let debug = format!("{secret:?}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "Debug leaked the secret value"
    );
    assert!(
        debug.contains("<redacted>"),
        "Debug must mark the value hidden"
    );

    let display = format!("{secret}");
    assert!(
        !display.contains("token-REDACTED-example"),
        "Display leaked the secret value"
    );

    // The same must hold for every type that holds a value.
    let key = SecretKey::new("workspace-1", "environment-2", "api_key")?;
    let store = MemorySecretStore::new();
    store.set(&key, &secret)?;
    let store_debug = format!("{store:?}");
    assert!(
        !store_debug.contains("token-REDACTED-example"),
        "Debug on a store holding the value leaked it"
    );

    Ok(())
}
