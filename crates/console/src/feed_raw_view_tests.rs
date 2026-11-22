//! The raw response view (REQ-CON-03, #113): `render_response` and `status_line` had no
//! coverage at all, so both wire forms are pinned here over the whole raw text — the
//! HTTP/1.x status line with its reason phrase, and the `:status` pseudo-header form the
//! HTTP/2 and HTTP/3 exchanges carry instead.
//!
//! `Decision:` the raw view keeps the wire's own forms (REQ-CON-03; S04/R04 for the
//! console's diagnostics, S05/R05 for response inspection): `HTTP/1.x <code> <reason>`
//! for HTTP/1.x — the reason trailing only when the wire carried one, so an empty phrase
//! leaves no dangling space — and `:status: <code>` for HTTP/2 and HTTP/3, which carry
//! pseudo-headers and no status line at all.

use api_model::SecretValues;
use http_engine::{Header, Response, ResponseBody, ResponseVersion};

/// A `200 OK` response carrying `body` and one header, on `version`.
fn response(version: ResponseVersion, body: &str) -> Response {
    Response {
        http_version: version,
        headers: vec![Header::new("Content-Type", "text/plain")],
        ..Response::new(200, "OK", ResponseBody::Bytes(body.as_bytes().to_vec()))
    }
}

/// The raw view of an HTTP/1.x response: the status line with its reason phrase, then
/// the headers and the body exactly as they arrived (REQ-CON-03).
#[test]
fn http_1x_response_raw_view_starts_with_the_status_line_and_reason() {
    let raw = super::render_response(
        &response(ResponseVersion::Http11, "hello"),
        &SecretValues::new(),
    );
    assert_eq!(
        raw,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nhello"
    );
}

/// The version on the status line is the one the response arrived on, and a reason
/// phrase the wire left empty leaves no dangling space after the code.
#[test]
fn http_1x_status_line_keeps_the_wire_version_and_drops_an_empty_reason() {
    let mut response = response(ResponseVersion::Http10, "");
    response.reason = String::new();
    assert_eq!(
        super::status_line(&response, &SecretValues::new()),
        "HTTP/1.0 200"
    );
}

/// The raw view of an HTTP/2 (or HTTP/3) response: the `:status` pseudo-header form the
/// exchange carries, not an HTTP/1.x status line the wire never wrote (REQ-CON-03).
#[test]
fn http_2_response_raw_view_shows_the_status_pseudo_header() {
    for version in [ResponseVersion::Http2, ResponseVersion::Http3] {
        let raw = super::render_response(&response(version, "hello"), &SecretValues::new());
        assert_eq!(raw, ":status: 200\r\nContent-Type: text/plain\r\n\r\nhello");
    }
}
