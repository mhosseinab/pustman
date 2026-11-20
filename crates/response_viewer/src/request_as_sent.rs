//! The "Request as sent" record (REQ-NET-04): what one send put on the wire — the final
//! URL, the method, every header it carried and a bounded look at the body — with every
//! value masked through `api_model::redact` (PRD §7). [`rows`] derives the record in one
//! pure pass; `timeline.rs` hosts it as the Timeline's "Request as sent" section and only
//! lays the rows out.
//!
//! `Decision:` the record is derived from the send's [`ResolvedRequest`] and
//! [`SecretValues`] (the `SendOutcome` pieces) plus the response and the jar's `Cookie`
//! value. Who calls
//! [`crate::timeline::TimelineTab::set_request_as_sent`] from the send path is later
//! wiring — nothing calls it yet, and `set_response` is unchanged.
//!
//! `Decision:` auth headers need no special handling here. `prepare` applies auth to
//! [`ResolvedRequest::headers`] (`http_engine::request_auth::set_header` replaces a
//! header of the same name), so `SendOutcome.request.headers` already carries the user's
//! and the auth-applied headers together and both list as ordinary rows.
//!
//! `Decision:` the jar's `Cookie` header never reaches `SendOutcome.request` — the
//! transport applies it per hop (`http_engine::cookie_transport`), after the request was
//! recorded — so the caller passes the value it computed (`CookieJar::cookie_header_for`)
//! as `cookie_header`, and [`cookie_row`] applies the transport's own precedence without
//! copying the jar's matching logic: a `Cookie` header the user wrote wins and the jar
//! adds nothing, and a send that disabled the jar sends none.
//!
//! `Decision:` the final URL is the location the last redirect hop was followed to when
//! the chain was followed (`Response::redirects`), else the URL the request went to —
//! the URL the response actually came from. The method and the headers stay those of the
//! request as sent; a redirect hop can change both on the wire (REQ-NET-03 lists the
//! hops), and this record describes the request the send built.
//!
//! `Decision:` masking: every value goes through `api_model::redact_*` with the send's
//! secrets, and `redact_headers` masks the credential headers (`Authorization`,
//! `Proxy-Authorization`, `Cookie` and the rest) in full whatever the secrets are — that
//! is relied on here, never reimplemented. Redaction runs before any truncation, so
//! cutting a secret in half can never leave a fragment the mask cannot match. A value
//! [`SecretValues`] refused to mask appears as it is (the caller warns the user), which
//! is why [`RequestAsSentRow`] keeps its rows redacted-only and prints no `Debug`.
//!
//! `Decision:` the body preview is bounded and never logged: a text body shows its first
//! [`BODY_PREVIEW_CHARS`] characters, redacted; a binary, file or form body shows a
//! one-line summary of what went out (type and size or field count) with no path, name
//! or field listed — a resolved variable can put a secret in any of them (REQ-VAR-01,
//! PRD §7). The preview is a summary, not the wire form (REQ-NET-04's "body as sent",
//! multipart boundaries included, is a later step's job).

use api_model::{SecretValues, redact_headers, redact_text, redact_url};
use gpui::SharedString;
use http_engine::{Header, RequestBody, ResolvedRequest, Response};

/// How much of a text body the preview shows before it stops and says so.
const BODY_PREVIEW_CHARS: usize = 200;

/// The `Cookie` header's name: the one the jar adds and a user row can pre-empt.
const COOKIE_HEADER: &str = "Cookie";

/// The summary rows' labels. A header can share one of these names; nothing looks rows
/// up by label at runtime — the labels name their own rows.
const METHOD_LABEL: &str = "Method";
const FINAL_URL_LABEL: &str = "Final URL";
const BODY_LABEL: &str = "Body";

/// One row of the "Request as sent" section: what the row names and the redacted value
/// shown beside it. Every value is built through `api_model::redact_*`, so a row holds
/// nothing a send put on the wire unmasked (PRD §7) — and, because a refused secret
/// survives redaction as it is, no [`Debug`](std::fmt::Debug) prints them either.
#[derive(Clone)]
pub struct RequestAsSentRow {
    /// What the row names: `Method`, `Final URL`, `Body`, or a header's name.
    pub label: SharedString,
    /// The redacted value shown beside the label.
    pub value: SharedString,
    /// A header the request's body mode generated on its own
    /// ([`ResolvedRequest::auto_headers`]), marked so the user can tell the send's
    /// additions from rows of their own (REQ-HTTP-03).
    pub auto: bool,
}

/// The rows of the "Request as sent" record for one send: the method, the final URL,
/// every header (`headers` first — the user's and the auth-applied ones in wire order —
/// then `auto_headers` marked auto, then the jar's `Cookie`), and the body preview.
/// Pure: no IO, no view state, no logging.
pub fn rows(
    request: &ResolvedRequest,
    secrets: &SecretValues,
    cookie_header: Option<&str>,
    response: &Response,
) -> Vec<RequestAsSentRow> {
    let mut rows = vec![
        RequestAsSentRow {
            label: SharedString::new_static(METHOD_LABEL),
            value: redact_text(request.method.as_str(), secrets).into(),
            auto: false,
        },
        RequestAsSentRow {
            label: SharedString::new_static(FINAL_URL_LABEL),
            value: redact_url(&final_url(request, response), secrets).into(),
            auto: false,
        },
    ];
    rows.extend(
        redact_headers(&header_pairs(&request.headers), secrets)
            .into_iter()
            .map(|(name, value)| header_row(name, value, false)),
    );
    rows.extend(
        redact_headers(&header_pairs(&request.auto_headers), secrets)
            .into_iter()
            .map(|(name, value)| header_row(name, value, true)),
    );
    rows.extend(cookie_row(request, secrets, cookie_header));
    rows.push(RequestAsSentRow {
        label: SharedString::new_static(BODY_LABEL),
        value: body_preview(&request.body, secrets),
        auto: false,
    });
    rows
}

/// One header row from an already-redacted `name: value` pair.
fn header_row(name: String, value: String, auto: bool) -> RequestAsSentRow {
    RequestAsSentRow {
        label: name.into(),
        value: value.into(),
        auto,
    }
}

/// The header rows exactly as `redact_headers` takes them.
fn header_pairs(headers: &[Header]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect()
}

/// The URL the response actually came from: the location the chain's last hop was
/// followed to (a hop is recorded only when the transport followed its `Location`), or
/// the request's own URL when nothing redirected.
fn final_url(request: &ResolvedRequest, response: &Response) -> String {
    response
        .redirects
        .last()
        .and_then(|hop| hop.location.as_ref())
        .map_or_else(
            || request.url.as_str().to_owned(),
            |location| location.as_str().to_owned(),
        )
}

/// The `Cookie` header row of the jar's value, or `None` when the send carried none of
/// it: the transport applies the header only when the user supplied no `Cookie` of their
/// own and the jar is enabled for the send (`cookie_transport::apply_cookie_header`).
fn cookie_row(
    request: &ResolvedRequest,
    secrets: &SecretValues,
    cookie_header: Option<&str>,
) -> Option<RequestAsSentRow> {
    let user_cookie = request
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(COOKIE_HEADER));
    if user_cookie || request.settings.disable_cookie_jar {
        return None;
    }
    let value = cookie_header?;
    // Through `redact_headers` like every other value: `Cookie` is a credential header,
    // so this masks it in full whatever the secrets are.
    let mut redacted = redact_headers(&[(COOKIE_HEADER.to_owned(), value.to_owned())], secrets);
    let (_, value) = redacted.pop()?;
    Some(header_row(COOKIE_HEADER.to_owned(), value, false))
}

/// A bounded look at the body as sent: the first [`BODY_PREVIEW_CHARS`] characters of a
/// text body, redacted, or a one-line summary of what else went out.
fn body_preview(body: &RequestBody, secrets: &SecretValues) -> SharedString {
    match body {
        RequestBody::None => SharedString::new_static("No body"),
        RequestBody::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok("") => SharedString::new_static("Empty body"),
            Ok(text) => text_preview(text, secrets),
            Err(_) => format!("Binary body, {} bytes", bytes.len()).into(),
        },
        // No path and no size: a resolved variable can put a secret in the path (PRD §7),
        // and a size needs a filesystem stat the foreground must not do.
        RequestBody::File(_) => SharedString::new_static("File body, streamed at send time"),
        // No field names: a resolved variable can put a secret in one (REQ-VAR-01).
        RequestBody::Multipart(fields) => format!("form-data body, {} fields", fields.len()).into(),
    }
}

/// The first [`BODY_PREVIEW_CHARS`] characters of `text`, redacted. Redaction runs on
/// the whole text before the truncation: cutting first could split a secret and leave a
/// fragment no mask matches.
fn text_preview(text: &str, secrets: &SecretValues) -> SharedString {
    let redacted = redact_text(text, secrets);
    let mut characters = redacted.chars();
    let preview: String = characters.by_ref().take(BODY_PREVIEW_CHARS).collect();
    if characters.next().is_some() {
        format!("{preview}…").into()
    } else {
        preview.into()
    }
}

#[cfg(test)]
#[path = "request_as_sent_tests.rs"]
mod tests;
