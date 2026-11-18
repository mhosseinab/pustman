//! Builders for the [`Variable`]s this crate's tests resolve.

use api_model::{Variable, VariableKind};
use std::collections::BTreeMap;

/// An enabled shared variable with a shared value.
pub(crate) fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// An enabled secret as a workspace file records it: no value at all
/// (REQ-VAR-SHARE-02).
pub(crate) fn secret(key: &str) -> Variable {
    Variable {
        kind: VariableKind::Secret,
        value: None,
        ..variable(key, "")
    }
}
