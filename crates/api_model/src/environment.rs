//! Environments: a named bag of variables resolved at send time (PRD §4, §7).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ItemId;
use crate::variable::Variable;

/// One environment (PRD §4): `Environment { id, name, variables }`.
///
/// Secret variables never carry a value here; each teammate supplies their own through the
/// keychain (REQ-VAR-SHARE-02).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    pub id: ItemId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
