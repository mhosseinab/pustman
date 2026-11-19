//! What goes on the wire once every field is resolved (PRD §5.1).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use url::Url;

use crate::error::TransportError;

/// One header field. Headers keep their order and their duplicates: a response may carry
/// several `Set-Cookie` fields and the order matters (REQ-RESP-05).
#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    /// The field name, as it goes on the wire.
    pub name: String,
    /// The field value. May carry a cookie, a token or a resolved secret.
    pub value: String,
}

impl Header {
    /// One header field.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Values carry cookies, tokens and resolved secrets: never print them.
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// The body to send: bytes, or a file the transport reads at send time (REQ-HTTP-04).
#[derive(Clone, PartialEq, Eq)]
pub enum RequestBody {
    /// No body at all.
    None,
    /// These bytes, exactly as they are.
    Bytes(Vec<u8>),
    /// A file streamed at send time (REQ-HTTP-05). A missing one fails the send with
    /// [`TransportError::FileNotFound`].
    File(PathBuf),
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Bodies never reach logs (AGENTS.md › Security).
        match self {
            RequestBody::None => f.write_str("None"),
            RequestBody::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            RequestBody::File(path) => f.debug_tuple("File").field(path).finish(),
        }
    }
}

/// What one send is allowed to do (REQ-HTTP-09): the global defaults already merged with
/// the request's overrides, so nothing is left to inherit at send time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendSettings {
    /// Give up after this long.
    pub timeout: Duration,
    /// Follow redirects at all.
    pub follow_redirects: bool,
    /// How many redirects to follow before [`TransportError::TooManyRedirects`].
    pub max_redirects: u32,
    /// On 301/302, keep the original method instead of switching to GET.
    pub follow_original_method: bool,
    /// Keep the `Authorization` header across redirects.
    pub keep_authorization: bool,
    /// Verify the server certificate (REQ-HTTP-10). Turning this off is a per-request
    /// choice the UI flags visibly.
    pub verify_tls: bool,
    /// The HTTP version this send allows.
    pub http_version: api_model::HttpVersion,
    /// Neither apply nor capture cookies for this send.
    pub disable_cookie_jar: bool,
    /// Percent-encode the URL automatically.
    pub encode_url: bool,
}

impl Default for SendSettings {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            follow_redirects: true,
            max_redirects: 10,
            follow_original_method: false,
            // Not kept: a redirect can cross hosts, and Authorization must not follow it
            // unless the request asked for that.
            keep_authorization: false,
            verify_tls: true,
            http_version: api_model::HttpVersion::Auto,
            disable_cookie_jar: false,
            encode_url: true,
        }
    }
}

/// A request with every field resolved: no `{{variables}}` left to substitute before it
/// goes on the wire.
#[derive(Clone, PartialEq)]
pub struct ResolvedRequest {
    pub method: api_model::Method,
    /// The URL as it is sent. Parsing it is part of building the request, so a bad one
    /// never reaches a server.
    pub url: Url,
    /// Headers in order, duplicates kept.
    pub headers: Vec<Header>,
    pub body: RequestBody,
    pub settings: SendSettings,
}

impl ResolvedRequest {
    /// Build the request to send from fields that already went through `{{variable}}`
    /// resolution. `url` is the resolved text.
    ///
    /// # Errors
    ///
    /// [`TransportError::InvalidUrl`] when `url` is not a valid URL — what variable
    /// resolution left behind, typically an unset variable (PRD §8.6).
    pub fn new(
        method: api_model::Method,
        url: &str,
        headers: Vec<Header>,
        body: RequestBody,
        settings: SendSettings,
    ) -> Result<Self, TransportError> {
        let parsed = Url::parse(url).map_err(|_| TransportError::InvalidUrl {
            url: url.to_string(),
        })?;
        Ok(Self {
            method,
            url: parsed,
            headers,
            body,
            settings,
        })
    }
}

impl fmt::Debug for ResolvedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The resolved text carries secret values (PRD §7): print no values at all.
        f.debug_struct("ResolvedRequest")
            .field("method", &self.method)
            .field("url", &crate::redacted_url(&self.url))
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("settings", &self.settings)
            .finish()
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
