//! Per-request overrides of the global request settings (PRD REQ-HTTP-09).
//!
//! Every field is optional and `None` means "inherit the global setting"; a request only
//! stores what it overrides.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The HTTP version a request is limited to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HttpVersion {
    /// Negotiate whatever the server offers.
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "1.1")]
    Http11,
    #[serde(rename = "2")]
    Http2,
}

/// Request settings: a global default exists for each field, and a request overrides it by
/// setting the field. `None` inherits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_redirects: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_original_method: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_tls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_url: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_referer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_version: Option<HttpVersion>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
