use super::SecretKey;
use crate::SecretError;

#[test]
fn secret_key_account_format_is_stable() -> Result<(), SecretError> {
    let key = SecretKey::new("workspace-1", "environment-2", "api_key")?;
    assert_eq!(key.account(), "workspace-1/environment-2/api_key");
    assert_eq!(key.to_string(), "workspace-1/environment-2/api_key");
    assert_eq!(
        SecretKey::SERVICE,
        "pustman",
        "every Pustman secret lives under one keychain service"
    );

    // The variable key is user text and may carry the separator; the account is a
    // rendering, not a path, and still belongs to exactly this key.
    let slashed = SecretKey::new("workspace-1", "environment-2", "api/v2/token")?;
    assert_eq!(slashed.account(), "workspace-1/environment-2/api/v2/token");

    // An account string only exists for keys the keychain can represent.
    assert!(matches!(
        SecretKey::new("", "environment-2", "api_key"),
        Err(SecretError::InvalidKey { .. })
    ));
    assert!(matches!(
        SecretKey::new("work/space", "environment-2", "api_key"),
        Err(SecretError::InvalidKey { .. })
    ));
    assert!(matches!(
        SecretKey::new("workspace-1", "environment-2", ""),
        Err(SecretError::InvalidKey { .. })
    ));
    Ok(())
}
