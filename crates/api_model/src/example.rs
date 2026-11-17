//! Saved examples: a request snapshot plus a named response (REQ-MODEL-04).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::request::{KeyValue, Request};

/// One saved example attached to a request, for documentation and sharing. Mock serving is
/// out of scope for v1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Example {
    pub name: String,
    pub request: Request,
    pub response: ExampleResponse,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The response an example captured.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExampleResponse {
    #[serde(default)]
    pub status: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: String,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
#[path = "example_tests.rs"]
mod tests;
