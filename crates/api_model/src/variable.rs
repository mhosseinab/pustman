//! Variables: `{{name}}` placeholders with shared values and never-shared secrets
//! (PRD §7, REQ-VAR-SHARE-01/02).

use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;

/// Who is allowed to see a variable's value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableKind {
    /// Shared with the team and written to the workspace file.
    #[default]
    Default,
    /// Never written to a file: each teammate supplies their own value from the OS keychain.
    Secret,
}

/// One `{{name}}` binding at some scope (collection, folder, environment or global).
///
/// A [`VariableKind::Secret`] variable never serializes its value and never shows it in
/// `Debug` output: the file records the key, kind and description but no value, so a repo
/// never carries a secret (REQ-VAR-SHARE-02).
#[derive(Clone, PartialEq, Deserialize)]
pub struct Variable {
    /// The `{{name}}` this variable resolves.
    pub key: String,
    /// The shared value, or the local value when the caller loaded one. Never serialized
    /// for a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub kind: VariableKind,
    #[serde(default = "crate::enabled_by_default")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl fmt::Debug for Variable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self.kind {
            VariableKind::Secret => self.value.as_deref().map(|_| "REDACTED"),
            VariableKind::Default => self.value.as_deref(),
        };
        f.debug_struct("Variable")
            .field("key", &self.key)
            .field("value", &value)
            .field("kind", &self.kind)
            .field("enabled", &self.enabled)
            .field("description", &self.description)
            .field("extra", &self.extra)
            .finish()
    }
}

impl Serialize for Variable {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("key", &self.key)?;
        if self.kind != VariableKind::Secret
            && let Some(value) = &self.value
        {
            map.serialize_entry("value", value)?;
        }
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("enabled", &self.enabled)?;
        if let Some(description) = &self.description {
            map.serialize_entry("description", description)?;
        }
        for (key, value) in &self.extra {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

#[cfg(test)]
#[path = "variable_tests.rs"]
mod tests;
