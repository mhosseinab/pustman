//! Redacted text export of Console entries (REQ-CON-05).
//!
//! [`export_text`] renders entries as plain text — one block per entry: the summary line,
//! then the raw request and the raw response. The export holds request and response
//! content — bodies, Authorization headers and cookies travel in it — so every piece of it
//! is redacted again even though the producer already redacted the entry: a producer that
//! forgets must not turn an export into a leak (defense in depth, REQ-NFR-04).
//!
//! Nothing here touches the disk (REQ-CON-06) and nothing here logs: this is a pure
//! function over strings, and the caller decides where the text goes.

use api_model::{SecretValues, redact_headers, redact_text, redact_url};

use crate::{ConsoleEntry, ConsoleLevel};

/// Export `entries` as redacted text, oldest first, one block per entry.
///
/// The summary line carries the entry's level, method or protocol, URL, status, time and
/// size; the raw request and the raw response follow it verbatim in shape but redacted.
/// Redaction is defense in depth: the URL runs through [`redact_url`], every free text
/// through [`redact_text`], and every `Name: value` line of a raw text's header block
/// through [`redact_headers`] so a credential header is masked whatever `secrets` knows.
/// The result is text for the caller to write wherever the user asked for it (REQ-CON-06:
/// this module itself never writes to disk).
pub fn export_text(entries: &[ConsoleEntry], secrets: &SecretValues) -> String {
    entries
        .iter()
        .map(|entry| export_entry(entry, secrets))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One entry's block: the summary line, then the raw request and the raw response when the
/// entry has them (a warning carries neither).
fn export_entry(entry: &ConsoleEntry, secrets: &SecretValues) -> String {
    let mut block = summary_line(entry, secrets);
    for (label, raw) in [
        ("request", &entry.raw_request),
        ("response", &entry.raw_response),
    ] {
        if raw.is_empty() {
            continue;
        }
        block.push('\n');
        block.push_str(&format!("--- {label} ---\n"));
        block.push_str(&redact_raw(raw, secrets));
    }
    block
}

/// The summary line: `[level] METHOD-OR-PROTOCOL URL status=… time=… size=…`, with the
/// fields the entry has. The URL goes through [`redact_url`] — it hides a percent-encoded
/// secret no plain pass can see — and the assembled line goes through [`redact_text`] so
/// no secret can be reassembled across the field boundaries this line draws.
fn summary_line(entry: &ConsoleEntry, secrets: &SecretValues) -> String {
    let level = match entry.level {
        ConsoleLevel::Info => "info",
        ConsoleLevel::Warning => "warning",
        ConsoleLevel::Error => "error",
    };
    let method_or_protocol = match &entry.method {
        Some(method) => method.to_string(),
        None => entry.protocol.clone(),
    };
    let mut summary = format!(
        "[{level}] {method_or_protocol} {}",
        redact_url(&entry.url, secrets)
    );
    if let Some(status) = entry.status {
        summary.push_str(&format!(" status={status}"));
    }
    if let Some(time) = entry.time {
        summary.push_str(&format!(" time={time:?}"));
    }
    if let Some(size) = entry.size {
        summary.push_str(&format!(" size={size} B"));
    }
    redact_text(&summary, secrets)
}

/// Redact one raw request or response text.
///
/// The head block — everything up to the first blank line — holds the start line and the
/// header lines; the body after it is free text. CRLF is folded to LF along the way: the
/// export is text for humans to read, not a byte-exact wire capture.
fn redact_raw(raw: &str, secrets: &SecretValues) -> String {
    let normalized = raw.replace("\r\n", "\n");
    let (head, body) = match normalized.split_once("\n\n") {
        Some((head, body)) => (head, Some(body)),
        None => (normalized.as_str(), None),
    };
    let mut text = redact_head(head, secrets);
    if let Some(body) = body {
        text.push_str("\n\n");
        // The body is redacted as one string so a secret that spans a line boundary is
        // still one occurrence.
        text.push_str(&redact_text(body, secrets));
    }
    text
}

/// Redact a head block: the start line on its own terms, every `Name: value` line under
/// the header rules, and anything else as free text.
fn redact_head(head: &str, secrets: &SecretValues) -> String {
    let mut redacted: Vec<String> = Vec::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut header_slots: Vec<usize> = Vec::new();
    for (index, line) in head.lines().enumerate() {
        let request_line = if index == 0 {
            redact_request_line(line, secrets)
        } else {
            None
        };
        if let Some(request_line) = request_line {
            redacted.push(request_line);
        } else if let Some((name, value)) = split_header_line(line) {
            headers.push((name, value));
            header_slots.push(redacted.len());
            redacted.push(String::new()); // replaced by the redacted header below
        } else {
            redacted.push(redact_text(line, secrets));
        }
    }
    // The header rules run on all header lines at once, exactly as on a request's headers.
    // The rebuilt line goes through [`redact_text`] once more: the `": "` this draws can
    // reassemble a secret that straddled the original name/value split.
    for (slot, (name, value)) in header_slots
        .into_iter()
        .zip(redact_headers(&headers, secrets))
    {
        redacted[slot] = redact_text(&format!("{name}: {value}"), secrets);
    }
    redacted.join("\n")
}

/// Redact a request line — `METHOD request-target HTTP/version` — running its target
/// through [`redact_url`] like the summary's URL. `None` when the line is not a request
/// line: a status line, a warning's text and HTTP/2 pseudo-headers are free text.
fn redact_request_line(line: &str, secrets: &SecretValues) -> Option<String> {
    let mut tokens = line.split_whitespace();
    let (method, target, version) = (tokens.next()?, tokens.next()?, tokens.next()?);
    if tokens.next().is_some() || !version.starts_with("HTTP/") {
        return None;
    }
    let target = redact_url(target, secrets);
    Some(redact_text(
        &format!("{method} {target} {version}"),
        secrets,
    ))
}

/// Split a line at its first `:` into header name and value. The name must be a non-empty
/// run without whitespace — that is the shape of an HTTP field name (RFC 9110 §5.1), so
/// prose with a colon in it keeps its spacing and stays free text. Both halves are
/// trimmed: optional whitespace frames a field value, and an indented credential header
/// must still be recognised as one.
fn split_header_line(line: &str) -> Option<(String, String)> {
    let (name, value) = line.split_once(':')?;
    let (name, value) = (name.trim(), value.trim());
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }
    Some((name.to_owned(), value.to_owned()))
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
