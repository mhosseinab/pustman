use crate::{SecretError, SecretKey};

/// A workspace id as long as the installation keys the app builds: the user-facing text
/// must not carry it.
const LONG_WORKSPACE_ID: &str = "01JD0000000000000000000000.0123456789abcdef0123456789abcdef";

fn key() -> SecretKey {
    match SecretKey::new(LONG_WORKSPACE_ID, "01JD0000000000000000000001", "api_token") {
        Ok(key) => key,
        Err(error) => panic!("the fixture key is valid: {error}"),
    }
}

#[test]
fn test_secret_errors_name_only_the_variable_key() {
    for error in [
        SecretError::NotFound { key: key() },
        SecretError::InvalidValue { key: key() },
        SecretError::UnusableSecret { key: key() },
    ] {
        let message = error.to_string();
        assert!(
            message.contains("`api_token`"),
            "the message names the variable: {message}"
        );
        assert!(
            !message.contains(LONG_WORKSPACE_ID) && !message.contains("01JD0000000000000000000001"),
            "the message carries no workspace or scope id: {message}"
        );
    }
}
