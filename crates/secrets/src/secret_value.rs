//! The value of a secret, with redacted output.

use std::fmt;

/// A secret's value: the text a secret variable resolves to.
///
/// Secrets are text. `Debug` and `Display` print `<redacted>`, so a value can never reach
/// a log line, a panic message or a test failure by accident; the only way to read the
/// text is [`SecretValue::expose`], and nothing in Pustman logs what that returns.
///
/// Dropping a `SecretValue` drops the text like any other string. There is deliberately no
/// custom `Drop`: a destructor that formats the value is exactly the leak this type
/// exists to prevent.
#[derive(Clone)]
pub struct SecretValue(String);

impl SecretValue {
    /// Wrap a secret's text.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret text. Callers must never log, print or persist this outside the keychain.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
#[path = "secret_value_tests.rs"]
mod tests;
