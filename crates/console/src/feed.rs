//! The Console feed (REQ-CON-01/03/04): turning sends, failures and cancellations into
//! [`ConsoleEntry`]s.
//!
//! The send's owner calls [`record_outcome`] or [`record_error`] when its task resolves,
//! and stores the [`Subscription`] [`watch`] returns to record cancellations
//! (REQ-HTTP-12). Every entry is redacted here — the send's secret values live only in
//! [`SendOutcome::secrets_used`] — with `api_model`'s shared masking (D8), and nothing in
//! this module logs anything (REQ-NFR-04).

use api_model::{HttpRequest, SecretValues, redact_headers, redact_text, redact_url};
use gpui::{App, Entity, Subscription};
use http_engine::{
    HttpClient, PipelineError, RequestBody, ResolvedRequest, Response, ResponseBody,
    ResponseVersion, SendEvent, SendOutcome, Warning,
};

use crate::store::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// The protocol name every entry carries, matching `storage`'s history rows: v1 sends
/// HTTP and nothing else (plan D7).
const PROTOCOL: &str = "http";

/// Record one finished send (REQ-CON-01): one entry holding the summary line's facts —
/// method, URL, status, time and size — and the raw request and response text exactly as
/// they went on the wire (REQ-CON-03), then one Warning entry per
/// [`Warning`] (REQ-CON-04) and one for every secret value that cannot be masked.
///
/// Everything is redacted with `outcome.secrets_used`: a secret value appears only as the
/// mask (REQ-NFR-04), and the entry's level is [`ConsoleLevel::Warning`] whenever the
/// send carries warnings at all.
pub fn record_outcome(outcome: &SendOutcome, cx: &mut App) {
    let secrets = &outcome.secrets_used;
    ConsoleStore::push(
        cx,
        ConsoleEntry {
            level: if outcome.warnings.is_empty() {
                ConsoleLevel::Info
            } else {
                ConsoleLevel::Warning
            },
            protocol: PROTOCOL.to_string(),
            method: Some(outcome.request.method.clone()),
            url: redact_url(outcome.request.url.as_str(), secrets),
            status: Some(outcome.response.status),
            time: outcome.response.timings.total,
            size: response_size(&outcome.response),
            raw_request: render_request(&outcome.request, outcome.response.http_version, secrets),
            raw_response: render_response(&outcome.response, secrets),
        },
    );
    for warning in &outcome.warnings {
        push_notice(
            cx,
            ConsoleLevel::Warning,
            &outcome.request,
            &warning_message(warning),
            secrets,
        );
    }
    // A refused secret value cannot be masked anywhere and will appear as it is, so the
    // refusal is not silent (`SecretValues::refused_count`).
    let refused = outcome.secrets_used.refused_count();
    if refused > 0 {
        let message = match refused {
            1 => "1 secret value cannot be masked and appears unmasked.".to_string(),
            count => format!("{count} secret values cannot be masked and appear unmasked."),
        };
        push_notice(
            cx,
            ConsoleLevel::Warning,
            &outcome.request,
            &message,
            secrets,
        );
    }
}

/// Record one failed send (REQ-CON-04) as an Error entry carrying the error's `Display`,
/// which is already redacted. A missing body file arrives as
/// [`PipelineError::Transport`] and lands here too (REQ-HTTP-05).
///
/// `request` is the unresolved request the caller sent, when it still has it: the entry
/// names the method and URL, and a failure before resolution leaves placeholders in the
/// URL as they were. Pass `None` when the request is gone.
pub fn record_error(request: Option<&HttpRequest>, error: &PipelineError, cx: &mut App) {
    // No secret was ever substituted on a failed send (or the caller no longer has the
    // values), so the empty set redacts the pre-resolution URL: userinfo and credential
    // shapes are masked whatever is known.
    let secrets = SecretValues::new();
    let (method, url) = match request {
        Some(request) => (
            Some(request.method.clone()),
            redact_url(&request.url, &secrets),
        ),
        None => (None, String::new()),
    };
    ConsoleStore::push(
        cx,
        ConsoleEntry {
            level: ConsoleLevel::Error,
            protocol: PROTOCOL.to_string(),
            method,
            url,
            status: None,
            time: None,
            size: None,
            raw_request: String::new(),
            raw_response: redact_text(&error.to_string(), &secrets),
        },
    );
}

/// What one [`Warning`] says to the user: [`Warning::user_message`], the one wording for
/// every entry that carries a warning, finished send or failed, and for the editor's own
/// hint.
fn warning_message(warning: &Warning) -> String {
    warning.user_message()
}

/// Record the warnings a failed send carried (REQ-VAR-07) as one Warning entry per
/// [`Warning`], the same wording [`record_outcome`] gives a finished send's. The caller
/// is a "Send anyway" failure: its unresolved names reach the Console whether the send
/// succeeded or not. With no warnings this records nothing.
///
/// `request` names the entries exactly as [`record_error`] names its own; pass `None`
/// when the request is gone. No secret was ever substituted on a failed send, so the
/// empty set redacts the pre-resolution URL and the message, as in [`record_error`].
pub fn record_warnings(request: Option<&HttpRequest>, warnings: &[Warning], cx: &mut App) {
    if warnings.is_empty() {
        return;
    }
    let secrets = SecretValues::new();
    let (method, url) = match request {
        Some(request) => (
            Some(request.method.clone()),
            redact_url(&request.url, &secrets),
        ),
        None => (None, String::new()),
    };
    for warning in warnings {
        ConsoleStore::push(
            cx,
            ConsoleEntry {
                level: ConsoleLevel::Warning,
                protocol: PROTOCOL.to_string(),
                method: method.clone(),
                url: url.clone(),
                status: None,
                time: None,
                size: None,
                raw_request: String::new(),
                raw_response: redact_text(&warning_message(warning), &secrets),
            },
        );
    }
}

/// Watch `client` and record every cancellation as an entry (REQ-HTTP-12). The caller
/// stores the returned [`Subscription`]; dropping it stops the recording.
pub fn watch(client: &Entity<HttpClient>, cx: &mut App) -> Subscription {
    cx.subscribe(client, |_, event: &SendEvent, cx| {
        // The entry names its own send from the event's identity (S33o: a replaced
        // send's cancellation is emitted after its history row is written, so the entry
        // can land after the new send's). The URL is redacted exactly as [`record_error`]
        // redacts one — the empty secret set masks userinfo and credential shapes — over
        // the URL as the send captured it: pre-resolution text where the send resolves
        // `{{variables}}` itself, and the scheme alone where the URL is resolved text
        // (`http_engine`'s capture rule: the resolved URL carries secret values).
        let secrets = SecretValues::new();
        let (method, url) = match &event.send {
            Some(send) => (Some(send.method.clone()), redact_url(&send.url, &secrets)),
            None => (None, String::new()),
        };
        ConsoleStore::push(
            cx,
            ConsoleEntry {
                level: ConsoleLevel::Warning,
                protocol: PROTOCOL.to_string(),
                method,
                url,
                status: None,
                time: None,
                size: None,
                raw_request: String::new(),
                raw_response: "Cancelled".to_string(),
            },
        );
    })
}

/// One entry that carries a message instead of wire text: its `raw_response` holds the
/// message — what came back from the send — and its `raw_request` stays empty. The
/// message is a notice, not a response: [`ConsoleEntry::notice`] is where consumers read
/// it, so the panel draws it under its own `Notice` section (#115).
fn push_notice(
    cx: &mut App,
    level: ConsoleLevel,
    request: &ResolvedRequest,
    message: &str,
    secrets: &SecretValues,
) {
    ConsoleStore::push(
        cx,
        ConsoleEntry {
            level,
            protocol: PROTOCOL.to_string(),
            method: Some(request.method.clone()),
            url: redact_url(request.url.as_str(), secrets),
            status: None,
            time: None,
            size: None,
            raw_request: String::new(),
            raw_response: redact_text(message, secrets),
        },
    );
}

/// The request exactly as it went on the wire (REQ-CON-03): `METHOD path?query <version>`,
/// then `Host:`, the headers and the body-mode auto headers in order, a blank line and
/// the body text. The `Host` line holds the Host header the wire carries — the user's own
/// when it set one, the URL's authority otherwise — so it is rendered once and never
/// duplicated in the header block. Line endings are the wire's CRLF. Redacted as one
/// rendering per part: the URL through [`redact_url`], the headers through
/// [`redact_headers`] and the free text through [`redact_text`].
///
/// `Decision:` the request line's version is the one the send actually used —
/// `version`, the response's `http_version`, which the transport reads off the wire —
/// not the request's `HttpVersion` setting, which says what the send allowed and leaves
/// `Auto` to negotiate (S04/R04). The request keeps request-line form in every version;
/// only the version token follows the exchange.
fn render_request(
    request: &ResolvedRequest,
    version: ResponseVersion,
    secrets: &SecretValues,
) -> String {
    let headers: Vec<(String, String)> = request
        .headers
        .iter()
        .chain(&request.auto_headers)
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect();
    let host = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map_or_else(|| host_line(&request.url), |(_, value)| value.clone());
    let mut raw = format!(
        "{} {} {}\r\n",
        request.method,
        redact_url(&request_target(&request.url), secrets),
        version_label(version)
    );
    raw.push_str(&format!("Host: {}\r\n", redact_text(&host, secrets)));
    let rest: Vec<(String, String)> = headers
        .into_iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("host"))
        .collect();
    push_headers(&mut raw, &rest, secrets);
    raw.push_str("\r\n");
    raw.push_str(&redact_text(&body_text(&request.body), secrets));
    raw
}

/// The response exactly as it arrived (REQ-CON-03): an HTTP/1.x status line, or the
/// `:status` pseudo-header form for HTTP/2 and HTTP/3, then the headers, a blank line and
/// the body text — redacted the same way [`render_request`] redacts a request.
fn render_response(response: &Response, secrets: &SecretValues) -> String {
    let mut raw = status_line(response, secrets);
    raw.push_str("\r\n");
    let headers: Vec<(String, String)> = response
        .headers
        .iter()
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect();
    push_headers(&mut raw, &headers, secrets);
    raw.push_str("\r\n");
    raw.push_str(&redact_text(
        &String::from_utf8_lossy(response.body.as_bytes()),
        secrets,
    ));
    raw
}

/// The status line of `response`: `:status: <code>` where the version speaks
/// pseudo-headers, `HTTP/1.x <code> <reason>` otherwise (REQ-CON-03).
fn status_line(response: &Response, secrets: &SecretValues) -> String {
    let reason = redact_text(&response.reason, secrets);
    match response.http_version {
        ResponseVersion::Http2 | ResponseVersion::Http3 => format!(":status: {}", response.status),
        ResponseVersion::Http10 | ResponseVersion::Http11 => {
            let version = version_label(response.http_version);
            // The wire's reason phrase may be empty; a trailing space would be noise.
            if reason.is_empty() {
                format!("{version} {}", response.status)
            } else {
                format!("{version} {} {}", response.status, reason)
            }
        }
    }
}

/// The version as text carries it in a request or status line (RFC 9110 §2.3).
fn version_label(version: ResponseVersion) -> &'static str {
    match version {
        ResponseVersion::Http10 => "HTTP/1.0",
        ResponseVersion::Http11 => "HTTP/1.1",
        ResponseVersion::Http2 => "HTTP/2",
        ResponseVersion::Http3 => "HTTP/3",
    }
}

/// Append `headers` to `raw` as `name: value` lines, redacted (credential headers masked
/// in full, everything else through [`redact_text`]).
fn push_headers(raw: &mut String, headers: &[(String, String)], secrets: &SecretValues) {
    for (name, value) in redact_headers(headers, secrets) {
        raw.push_str(&format!("{name}: {value}\r\n"));
    }
}

/// The request target on the wire: the path and the query, never the fragment (RFC 9110
/// §4.2 — it is not sent) and never the userinfo (it goes in `Authorization`, if at all).
fn request_target(url: &url::Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_string(),
    }
}

/// The `Host` header value for `url`: the host, with the port when one is explicit and
/// non-default (RFC 9110 §7.2).
fn host_line(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// The request body as text. Bytes render lossily — the raw view is text — and a file or
/// multipart body renders as a placeholder: the transport reads the bytes at send time
/// and chooses the multipart framing, so the exact wire text is not knowable here.
fn body_text(body: &RequestBody) -> String {
    match body {
        RequestBody::None => String::new(),
        RequestBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        RequestBody::File(path) => format!("<file body: {}>", path.display()),
        RequestBody::Multipart(fields) => format!("<multipart body: {} fields>", fields.len()),
    }
}

/// The response size in bytes: the whole body, or what the raw view can show of a body
/// that streamed past the in-memory prefix.
fn response_size(response: &Response) -> Option<u64> {
    let size = match &response.body {
        ResponseBody::Spilled { total_len, .. } => *total_len,
        body => u64::try_from(body.as_bytes().len()).unwrap_or(u64::MAX),
    };
    Some(size)
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "feed_order_tests.rs"]
mod feed_order_tests;

#[cfg(test)]
#[path = "feed_raw_view_tests.rs"]
mod raw_view_tests;

#[cfg(test)]
#[path = "feed_version_tests.rs"]
mod version_tests;
