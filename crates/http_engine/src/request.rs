//! What goes on the wire once every field is resolved (PRD §5.1).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use reqwest::header::HeaderMap;
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
        // Values carry cookies, tokens and resolved secrets: never print them. Names can
        // too — variables resolve into header names as well (PRD §7) — so only a
        // well-known header name prints as-is; any other name is redacted like a value.
        let name = if is_well_known_header_name(&self.name) {
            self.name.as_str()
        } else {
            "<redacted>"
        };
        f.debug_struct("Header")
            .field("name", &name)
            .field("value", &"<redacted>")
            .finish()
    }
}

/// The standard wire names [`fmt::Debug for Header`](Header) prints as-is. Anything
/// else may be a name `{{variable}}` resolution built from a secret (PRD §7), so the
/// names come from the `http::header` constants the wire format standardises and are
/// matched ASCII-case-insensitively, as header names are.
fn is_well_known_header_name(name: &str) -> bool {
    use reqwest::header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONNECTION,
        CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, DATE, ETAG, HOST,
        IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION, ORIGIN, PROXY_AUTHORIZATION,
        REFERER, SERVER, SET_COOKIE, TRANSFER_ENCODING, USER_AGENT, VARY, WWW_AUTHENTICATE,
    };

    [
        AUTHORIZATION,
        CONTENT_TYPE,
        CONTENT_LENGTH,
        CONTENT_ENCODING,
        ACCEPT,
        ACCEPT_ENCODING,
        ACCEPT_LANGUAGE,
        USER_AGENT,
        HOST,
        COOKIE,
        SET_COOKIE,
        LOCATION,
        CACHE_CONTROL,
        CONNECTION,
        DATE,
        ETAG,
        IF_NONE_MATCH,
        IF_MODIFIED_SINCE,
        LAST_MODIFIED,
        ORIGIN,
        REFERER,
        TRANSFER_ENCODING,
        WWW_AUTHENTICATE,
        PROXY_AUTHORIZATION,
        SERVER,
        VARY,
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known.as_str()))
}

/// The header fields of `map`, duplicates kept (REQ-RESP-05): `http::HeaderMap` yields
/// a name once per value. Duplicates of one name keep their relative order; the order
/// across different names is not guaranteed (see issue #31).
pub(crate) fn headers_from_map(map: &HeaderMap) -> Vec<Header> {
    map.iter()
        .map(|(name, value)| {
            Header::new(
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// One `form-data` field as the transport sends it (REQ-HTTP-04): a text value, or a
/// file read at send time (REQ-HTTP-05).
#[derive(Clone, PartialEq, Eq)]
pub struct MultipartField {
    /// The field name.
    pub name: String,
    /// The content type this field carries, when the request set one.
    pub content_type: Option<String>,
    pub value: MultipartValue,
}

/// What one [`MultipartField`] sends.
#[derive(Clone, PartialEq, Eq)]
pub enum MultipartValue {
    /// This text, exactly as it is.
    Text(String),
    /// A file read at send time (REQ-HTTP-05). A missing one fails the send with
    /// [`TransportError::FileNotFound`].
    File(PathBuf),
}

/// The body to send: bytes, a file the transport reads at send time (REQ-HTTP-04), or
/// the fields of a `form-data` body (REQ-HTTP-04/05).
#[derive(Clone, PartialEq, Eq)]
pub enum RequestBody {
    /// No body at all.
    None,
    /// These bytes, exactly as they are.
    Bytes(Vec<u8>),
    /// A file streamed at send time (REQ-HTTP-05). A missing one fails the send with
    /// [`TransportError::FileNotFound`].
    File(PathBuf),
    /// `form-data` fields, framed by the transport as a multipart message.
    Multipart(Vec<MultipartField>),
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Bodies never reach logs (AGENTS.md › Security) and a file's path can carry a
        // resolved secret (REQ-VAR-01 resolves `{{variables}}` in the body, PRD §7): it
        // is redacted exactly as [`crate::error::TransportError::FileNotFound`] redacts
        // one (`error.rs`). The UI keeps the real path — the body editor holds it and
        // `FileNotFound`'s `user_message()` names it. A multipart body shows its field
        // COUNT and nothing else: a field name is as secret-bearing as a value or a file
        // path, because variable resolution builds names too (REQ-VAR-01, PRD §7).
        match self {
            RequestBody::None => f.write_str("None"),
            RequestBody::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            RequestBody::File(_) => f.debug_struct("File").field("path", &"<redacted>").finish(),
            RequestBody::Multipart(fields) => f
                .debug_struct("Multipart")
                .field("fields", &fields.len())
                .finish(),
        }
    }
}

/// [`RequestBody`] as [`ResolvedRequest`]'s `Debug` prints it. [`RequestBody`]'s own
/// `Debug` is stricter — the path fully redacted — but the file NAME survives here,
/// and only here, because `request_tests.rs` (`debug_output_describes_a_file_body_by_path`)
/// pins it visible and that test file is read-only on the integration branch. The
/// directory, where a resolved variable puts its secret (PRD §7), never appears.
struct BodyForDebug<'a>(&'a RequestBody);

impl fmt::Debug for BodyForDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            RequestBody::None => f.write_str("None"),
            RequestBody::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            RequestBody::Multipart(fields) => f
                .debug_struct("Multipart")
                .field("fields", &fields.len())
                .finish(),
            RequestBody::File(path) => match path.file_name() {
                Some(name) => f
                    .debug_tuple("File")
                    .field(&name.to_string_lossy())
                    .finish(),
                None => f.debug_tuple("File").field(&"<redacted>").finish(),
            },
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
    /// The headers the request's body mode adds on its own — listed for the UI
    /// (REQ-HTTP-03), not sent as they are listed: see `body::auto_headers` for the
    /// rules and `reqwest_transport` for what reaches the wire.
    pub auto_headers: Vec<Header>,
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
            auto_headers: Vec::new(),
            body,
            settings,
        })
    }
}

impl fmt::Debug for ResolvedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The resolved text carries secret values (PRD §7): print no values at all. The
        // URL keeps its scheme, host and path and a body file keeps its NAME here, and
        // only here: `request_tests.rs` pins exactly that visible and is read-only on
        // the integration branch (see [`debug_url`] and [`BodyForDebug`]).
        f.debug_struct("ResolvedRequest")
            .field("method", &self.method)
            .field("url", &debug_url(&self.url))
            .field("headers", &self.headers)
            .field("auto_headers", &self.auto_headers)
            .field("body", &BodyForDebug(&self.body))
            .field("settings", &self.settings)
            .finish()
    }
}

/// The URL as [`ResolvedRequest`]'s `Debug` prints it: scheme, host and path, with the
/// userinfo, query and fragment dropped. This is the crate's one deviation from
/// [`crate::redacted_url`] — which drops the host and path as well — and it exists only
/// because `request_tests.rs` (`debug_output_hides_bodies_headers_and_query_values`,
/// `debug_output_hides_url_userinfo_credentials`) pins host and path visible in a test
/// file that is read-only on the integration branch.
fn debug_url(url: &Url) -> String {
    let mut shown = url.clone();
    shown.set_query(None);
    shown.set_fragment(None);
    // Userinfo carries credentials exactly as the query does. `set_username` and
    // `set_password` refuse a URL with no host (or a `file:` one), which is exactly the
    // set of URLs that carry no userinfo to clear, so a refusal is already redacted.
    let userinfo_cleared = shown.set_username("").and(shown.set_password(None));
    debug_assert!(
        userinfo_cleared.is_ok() || !shown.has_host() || shown.scheme() == "file",
        "a URL with a host accepts an empty userinfo"
    );
    shown.to_string()
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "multipart_debug_redaction_tests.rs"]
mod multipart_debug_redaction_tests;
