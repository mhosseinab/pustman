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
    /// Keep the `Authorization` header across redirects (REQ-HTTP-09). Off by default:
    /// a redirect can cross hosts, and credentials must not follow it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_authorization: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_tls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encode_url: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_referer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_version: Option<HttpVersion>,
    /// Neither apply nor capture cookies for this send (REQ-HTTP-09): the cookie jar
    /// is off for this request alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_cookie_jar: Option<bool>,
    /// The auto headers this request leaves out of the send (REQ-HTTP-03): the names of
    /// generated headers — `Host`, `Content-Type`, … — the user switched off. An empty
    /// list is the default and never serializes, so the deterministic workspace format
    /// is unchanged for requests that use none of it. Names compare
    /// ASCII-case-insensitively, like every header name (RFC 9110 §5.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_auto_headers: Vec<String>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl RequestSettings {
    /// Whether `name` names an auto header this request does not send (REQ-HTTP-03).
    /// Header names compare ASCII-case-insensitively (RFC 9110 §5.1).
    pub fn is_auto_header_disabled(&self, name: &str) -> bool {
        self.disabled_auto_headers
            .iter()
            .any(|disabled| disabled.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "settings_disabled_auto_tests.rs"]
mod disabled_auto_tests;

#[cfg(test)]
#[path = "settings_req_http_09_tests.rs"]
mod req_http_09_tests;
