//! Identity of one secret in the keychain.

use std::fmt;

use crate::SecretError;

/// Names one secret: the workspace it belongs to, the variable scope it lives in and the
/// variable's own key.
///
/// A key is stored under the keychain service [`SecretKey::SERVICE`] with the account
/// [`SecretKey::account`] renders. The account is an identifier that is never parsed back.
///
/// Keys are validated when they are built, so every `SecretKey` is representable in the
/// keychain and no store has to second-guess one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SecretKey {
    workspace_id: String,
    scope_id: String,
    variable_key: String,
}

impl SecretKey {
    /// The keychain service every Pustman secret is stored under.
    pub const SERVICE: &'static str = "pustman";

    /// Name a secret.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidKey`] when a component is empty or contains a NUL byte, or
    /// when `workspace_id` or `scope_id` contains `/`. Those two are machine-made ids (an
    /// item id, a scope name) and `/` separates the parts of the account rendering, so an
    /// id carrying one would let two different keys render as the same account.
    /// `variable_key` is user text and may contain `/` — `api/v2/token` is a legal
    /// variable name — which stays unambiguous because the two id components are
    /// separator-free.
    pub fn new(
        workspace_id: impl Into<String>,
        scope_id: impl Into<String>,
        variable_key: impl Into<String>,
    ) -> Result<Self, SecretError> {
        let key = Self {
            workspace_id: workspace_id.into(),
            scope_id: scope_id.into(),
            variable_key: variable_key.into(),
        };
        key.validate()?;
        Ok(key)
    }

    /// The workspace the secret belongs to.
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// The variable scope (an environment, the globals, ...) the secret belongs to.
    pub fn scope_id(&self) -> &str {
        &self.scope_id
    }

    /// The variable's own key within its scope.
    pub fn variable_key(&self) -> &str {
        &self.variable_key
    }

    /// The keychain account this key renders as: `workspace_id/scope_id/variable_key`.
    pub fn account(&self) -> String {
        self.to_string()
    }

    fn validate(&self) -> Result<(), SecretError> {
        for (name, value) in [
            ("workspace_id", self.workspace_id.as_str()),
            ("scope_id", self.scope_id.as_str()),
            ("variable_key", self.variable_key.as_str()),
        ] {
            if value.is_empty() {
                return Err(SecretError::InvalidKey {
                    reason: format!("{name} is empty"),
                });
            }
            if value.contains('\0') {
                return Err(SecretError::InvalidKey {
                    reason: format!("{name} contains a NUL byte"),
                });
            }
        }
        for (name, value) in [
            ("workspace_id", self.workspace_id.as_str()),
            ("scope_id", self.scope_id.as_str()),
        ] {
            if value.contains('/') {
                return Err(SecretError::InvalidKey {
                    reason: format!(
                        "{name} must not contain '/': it separates the parts of the keychain account"
                    ),
                });
            }
        }
        Ok(())
    }
}

impl fmt::Display for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}/{}",
            self.workspace_id, self.scope_id, self.variable_key
        )
    }
}

#[cfg(test)]
#[path = "secret_key_tests.rs"]
mod tests;
