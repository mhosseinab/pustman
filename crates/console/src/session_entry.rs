//! One Console entry per protocol session (PRD §9, REQ-WS-02): the target, the redacted
//! handshake, how many messages went each way and how big they were, and why the session
//! ended.
//!
//! A [`SessionRecorder`] is fed a session's [`SessionEvent`]s as they happen and keeps one
//! [`ConsoleEntry`] up to date; the session's owner pushes it once, when the session ends.
//! Payloads are never copied — only counts and sizes — and everything that can carry a
//! secret is redacted with `api_model`'s shared masking (D8, REQ-NFR-04) before it is kept.

use api_model::{
    CloseReason, ConnectionDetails, SecretValues, SessionEvent, redact_text, redact_url,
};
use gpui::App;

use crate::store::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// The most characters of server-supplied free text — an error message or a close reason —
/// the entry keeps. A server can echo arbitrary data there; the Console needs the gist.
const MAX_REASON_CHARS: usize = 512;

/// Handshake headers whose value is a token by convention but which the shared credential
/// list does not mask whole: browsers cannot set `Authorization` on a WebSocket upgrade, so
/// servers take the token as a subprotocol (#338).
const TOKEN_CARRYING_HEADERS: [&str; 1] = ["sec-websocket-protocol"];

/// gRPC metadata keys ending in this carry binary values (gRPC over HTTP/2 spec), usually
/// signed tokens or serialized credentials (#338).
const BINARY_METADATA_SUFFIX: &str = "-bin";

/// Messages and bytes one direction carried.
#[derive(Clone, Copy, Default)]
struct Tally {
    messages: u64,
    bytes: u64,
}

impl Tally {
    fn add(&mut self, size: u64) {
        self.messages = self.messages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(size);
    }

    fn line(self, label: &str) -> String {
        let noun = if self.messages == 1 {
            "message"
        } else {
            "messages"
        };
        format!(
            "{label}: {} {noun}, {} bytes\r\n",
            self.messages, self.bytes
        )
    }
}

/// Builds the one [`ConsoleEntry`] a session leaves (REQ-WS-02).
///
/// Everything kept is already redacted: the recorder holds the caller's secret values, never
/// the raw handshake, error or close text.
pub struct SessionRecorder {
    entry: ConsoleEntry,
    secrets: SecretValues,
    /// The redacted handshake, once the session connected.
    handshake: Option<ConnectionDetails>,
    /// How many handshake values that look like tokens could not be masked (too short, or
    /// holding the mask character); the entry says so, as the HTTP feed does.
    unmaskable_handshake_values: usize,
    sent: Tally,
    received: Tally,
    /// The first error, redacted and capped: later ones are usually its consequences.
    error: Option<String>,
    /// Why the session ended, redacted and capped.
    close: Option<String>,
}

impl SessionRecorder {
    /// A recorder for a session of `protocol` (the label History and the Console share:
    /// `websocket`, `socketio`, `grpc`, `sse`, `graphql`) to `target`, redacting with
    /// `secrets` — the session's secret values, which only its owner knows.
    ///
    /// `Decision:` the secrets are a constructor argument rather than optional, so a
    /// producer cannot forget them and leak a resolved token into the Console.
    pub fn new(protocol: &str, target: &str, secrets: SecretValues) -> Self {
        let mut recorder = Self {
            entry: ConsoleEntry {
                level: ConsoleLevel::Info,
                protocol: protocol.to_string(),
                method: None,
                url: redact_url(target, &secrets),
                status: None,
                time: None,
                size: None,
                raw_request: String::new(),
                raw_response: String::new(),
            },
            secrets,
            handshake: None,
            unmaskable_handshake_values: 0,
            sent: Tally::default(),
            received: Tally::default(),
            error: None,
            close: None,
        };
        recorder.render();
        recorder
    }

    /// Fold one event into the entry.
    pub fn record(&mut self, event: &SessionEvent) {
        match event {
            // The target was fixed at construction; a reconnect to the same target adds
            // nothing the entry does not already say.
            SessionEvent::Connecting { .. } => {}
            SessionEvent::Connected { details } => self.record_handshake(details),
            SessionEvent::Sent(message) => self.sent.add(message.size),
            SessionEvent::Received(message) => self.received.add(message.size),
            SessionEvent::Error(error) => {
                self.raise(ConsoleLevel::Error);
                if self.error.is_none() {
                    self.error = Some(self.server_text(&error.message));
                }
            }
            SessionEvent::Closed { reason } => {
                if matches!(reason, CloseReason::Lost) {
                    self.raise(ConsoleLevel::Warning);
                }
                self.close = Some(self.close_text(reason));
            }
        }
        self.render();
    }

    /// The entry as it stands.
    pub fn entry(&self) -> &ConsoleEntry {
        &self.entry
    }

    /// Push the session's entry into the Console. Consumes the recorder: a session leaves
    /// exactly one entry.
    pub fn push(self, cx: &mut App) {
        ConsoleStore::push(cx, self.entry);
    }

    fn record_handshake(&mut self, details: &ConnectionDetails) {
        // Token-carrying values join the secret set rather than being masked here, so the
        // shared redaction masks them wherever they appear — in the offering header, in the
        // server's echo and in the negotiated subprotocol.
        let mut secrets = self.secrets.clone();
        let mut unmaskable = 0;
        for (name, value) in details
            .request_headers
            .iter()
            .chain(&details.response_headers)
        {
            if !carries_token(name) {
                continue;
            }
            for token in value
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                if !secrets.insert(token) {
                    unmaskable += 1;
                }
            }
        }
        self.unmaskable_handshake_values = unmaskable;
        self.entry.status = details.response_status;
        if let Some(status) = details.response_status
            && refused_upgrade(status)
        {
            self.raise(ConsoleLevel::Error);
        }
        self.handshake = Some(details.redacted(&secrets));
    }

    fn raise(&mut self, level: ConsoleLevel) {
        let rank = |level: ConsoleLevel| match level {
            ConsoleLevel::Info => 0,
            ConsoleLevel::Warning => 1,
            ConsoleLevel::Error => 2,
        };
        if rank(level) > rank(self.entry.level) {
            self.entry.level = level;
        }
    }

    /// Server-supplied free text, redacted first so the cap can never leave half a secret.
    fn server_text(&self, text: &str) -> String {
        capped(&redact_text(text, &self.secrets))
    }

    fn close_text(&self, reason: &CloseReason) -> String {
        match reason {
            CloseReason::Client => "Closed by the client".to_string(),
            CloseReason::Lost => "Connection lost without a close handshake".to_string(),
            CloseReason::Server { code, reason } => {
                let mut text = match code {
                    Some(code) => format!("Closed by the server (code {code})"),
                    None => "Closed by the server".to_string(),
                };
                if !reason.is_empty() {
                    text.push_str(": ");
                    text.push_str(&self.server_text(reason));
                }
                text
            }
        }
    }

    /// Rebuild the entry's size and raw text from what has been recorded.
    fn render(&mut self) {
        let mut request = format!("{}\r\n", self.entry.url);
        let mut response = String::new();
        let mut body_size = None;
        if let Some(handshake) = &self.handshake {
            push_headers(&mut request, &handshake.request_headers);
            if let Some(status) = handshake.response_status {
                response.push_str(&format!("Status: {status}\r\n"));
                if refused_upgrade(status) {
                    body_size = content_length(&handshake.response_headers);
                }
            }
            if let Some(protocol) = &handshake.protocol {
                response.push_str(&format!("Protocol: {protocol}\r\n"));
            }
            push_headers(&mut response, &handshake.response_headers);
            response.push_str("\r\n");
            if let Some(size) = body_size {
                response.push_str(&format!("Body: {size} bytes\r\n"));
            }
            if self.unmaskable_handshake_values > 0 {
                response.push_str(&format!(
                    "{} handshake values cannot be masked and appear unmasked.\r\n",
                    self.unmaskable_handshake_values
                ));
            }
        }
        response.push_str(&self.sent.line("Sent"));
        response.push_str(&self.received.line("Received"));
        if let Some(error) = &self.error {
            response.push_str(&format!("Error: {error}\r\n"));
        }
        if let Some(close) = &self.close {
            response.push_str(&format!("{close}\r\n"));
        }
        // A refused upgrade's size is the body the server sent instead; an open
        // session's is what it received.
        self.entry.size = if self.entry.status.is_some_and(refused_upgrade) {
            body_size
        } else if self.handshake.is_some() || self.received.messages > 0 {
            Some(self.received.bytes)
        } else {
            None
        };
        self.entry.raw_request = request;
        self.entry.raw_response = response;
    }
}

/// Whether a handshake status means the server did not open the session: anything but
/// `101 Switching Protocols` (WebSocket, Socket.IO) or a 2xx (SSE, gRPC, GraphQL over HTTP).
fn refused_upgrade(status: u16) -> bool {
    status != 101 && !(200..300).contains(&status)
}

/// Whether a header's value is a token by convention (see [`TOKEN_CARRYING_HEADERS`] and
/// [`BINARY_METADATA_SUFFIX`]). Matched on visible ASCII, case-insensitively, as the shared
/// credential-header list matches names.
fn carries_token(name: &str) -> bool {
    let visible: String = name
        .chars()
        .filter(char::is_ascii_graphic)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    TOKEN_CARRYING_HEADERS.contains(&visible.as_str()) || visible.ends_with(BINARY_METADATA_SUFFIX)
}

/// The body size a refused upgrade's response declared.
fn content_length(headers: &[(String, String)]) -> Option<u64> {
    headers
        .iter()
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
}

/// Append already-redacted `headers` as `name: value` lines.
fn push_headers(raw: &mut String, headers: &[(String, String)]) {
    for (name, value) in headers {
        raw.push_str(&format!("{name}: {value}\r\n"));
    }
}

/// `text` cut to [`MAX_REASON_CHARS`] characters, marked with an ellipsis when cut.
fn capped(text: &str) -> String {
    match text.char_indices().nth(MAX_REASON_CHARS) {
        Some((end, _)) => format!("{}…", text.get(..end).unwrap_or_default()),
        None => text.to_string(),
    }
}

#[cfg(test)]
#[path = "session_entry_tests.rs"]
mod tests;
