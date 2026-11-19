//! Encoding a request body for the wire, and the headers a body mode describes itself
//! with (REQ-HTTP-03/04/05).
//!
//! The auto headers are two things at once: the list the UI shows read-only
//! ([`auto_headers`] fills [`crate::ResolvedRequest::auto_headers`]) and the few fields
//! the transport actually sends (`reqwest_transport` sends `Content-Type`, `User-Agent`
//! and `Accept`, and leaves `Host`, `Content-Length` and `Accept-Encoding` to
//! hyper/reqwest). A user header of the same name wins over any auto one, in both.

use std::path::{Path, PathBuf};

use api_model::RawLanguage;
use url::Url;

use crate::error::TransportError;
use crate::file_ref::resolve_file_ref;
use crate::request::{Header, MultipartField, MultipartValue, RequestBody};
use crate::reqwest_transport::authority_of;

/// The `Accept-Encoding` value of the `reqwest` client's own automatic header: the
/// encodings it decompresses, as `gzip`, `brotli` and `deflate` are the ones the root
/// `Cargo.toml` enables. The transport does not send this listing — `reqwest` (through
/// tower-http's `Decompression`) adds exactly this value when the request has none, and
/// an explicit one would override the client's own set as features change.
const ACCEPT_ENCODING: &str = "gzip,deflate,br";

/// The auto headers of this request, in the fixed order the UI lists them (REQ-HTTP-03):
/// `Content-Type`, `Content-Length`, `User-Agent`, `Accept`, `Accept-Encoding`, `Host`.
/// The order is part of the listing — nothing here reads a map, so it never jitters.
///
/// `body` is the unresolved body: it carries the mode and the raw language, which is
/// what names the `Content-Type`. `resolved_body` is what goes on the wire, and its
/// `Content-Length` is listed only when the length is known here — the bytes are in
/// hand. A file or multipart length is decided at send time (multipart framing carries
/// its own), so no length is listed for one. A user header of the same name (compared
/// ASCII-case-insensitively) suppresses the auto one entirely.
pub fn auto_headers(
    body: &api_model::Body,
    resolved_body: &RequestBody,
    url: &Url,
    user_headers: &[Header],
) -> Vec<Header> {
    let mut headers = Vec::new();
    if let Some(content_type) = content_type_of(body) {
        headers.push(Header::new("Content-Type", content_type));
    }
    if let RequestBody::Bytes(bytes) = resolved_body {
        headers.push(Header::new("Content-Length", bytes.len().to_string()));
    }
    headers.push(Header::new(
        "User-Agent",
        format!("Pustman/{}", env!("CARGO_PKG_VERSION")),
    ));
    headers.push(Header::new("Accept", "*/*"));
    headers.push(Header::new("Accept-Encoding", ACCEPT_ENCODING));
    headers.push(Header::new("Host", authority_of(url)));
    headers.retain(|header| !user_header_is_set(user_headers, &header.name));
    headers
}

/// Whether `user_headers` carries `name` — the user's header wins over an auto one of
/// the same name, and header names compare ASCII-case-insensitively (RFC 9110 §5.1).
pub(crate) fn user_header_is_set(user_headers: &[Header], name: &str) -> bool {
    user_headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
}

/// The `Content-Type` a body mode describes itself with (REQ-HTTP-03/04). A request
/// with no body — or one no mode models — describes none.
fn content_type_of(body: &api_model::Body) -> Option<&'static str> {
    match body {
        api_model::Body::None | api_model::Body::Unsupported(_) => None,
        api_model::Body::FormData { .. } => Some("multipart/form-data"),
        api_model::Body::UrlEncoded { .. } => Some("application/x-www-form-urlencoded"),
        api_model::Body::Raw { language, .. } => Some(match language {
            RawLanguage::Text => "text/plain",
            RawLanguage::Json => "application/json",
            RawLanguage::Xml => "application/xml",
            RawLanguage::Html => "text/html",
            RawLanguage::JavaScript => "application/javascript",
        }),
        api_model::Body::Binary { .. } => Some("application/octet-stream"),
    }
}

/// Resolve every body file reference of `body` against `workspace_root` (REQ-HTTP-05):
/// a file body's path and every multipart file field's. A reference escaping the root is
/// refused ([`crate::file_ref::resolve_file_ref`]); anything without a file keeps its
/// text as resolved.
///
/// The filesystem calls are blocking: `send_request` runs this on the background task,
/// never on the GPUI foreground thread.
pub fn resolve_body_files(
    body: RequestBody,
    workspace_root: &Path,
) -> Result<RequestBody, TransportError> {
    match body {
        RequestBody::File(path) => Ok(RequestBody::File(resolve_path(path, workspace_root)?)),
        RequestBody::Multipart(fields) => {
            let mut resolved = Vec::with_capacity(fields.len());
            for field in fields {
                let MultipartField {
                    name,
                    content_type,
                    value,
                } = field;
                let value = match value {
                    MultipartValue::File(path) => {
                        MultipartValue::File(resolve_path(path, workspace_root)?)
                    }
                    text => text,
                };
                resolved.push(MultipartField {
                    name,
                    content_type,
                    value,
                });
            }
            Ok(RequestBody::Multipart(resolved))
        }
        body => Ok(body),
    }
}

/// [`resolve_file_ref`] over one path: the reference as the model stored it, which is
/// always valid UTF-8 — it arrived as a `String`.
fn resolve_path(path: PathBuf, workspace_root: &Path) -> Result<PathBuf, TransportError> {
    let reference = path.to_str().ok_or_else(|| TransportError::Io {
        detail: "The body file reference is not valid UTF-8.".to_string(),
    })?;
    resolve_file_ref(workspace_root, reference)
}

#[cfg(test)]
#[path = "body_tests.rs"]
mod tests;
