//! The shared streaming model (PRD §9): every session protocol (WebSocket, Socket.IO, SSE,
//! gRPC streams) reports what happens as [`SessionEvent`]s, so the Message log and the Console
//! stay protocol-agnostic (REQ-LOG-01/02).
//!
//! Plain data: no IO, no GPUI. A payload too large to keep in memory is spilled to a file by
//! the session store; this module only describes it, and an export reads the spilled bytes
//! back in a later step.

use std::fmt;
use std::path::PathBuf;

use base64::Engine;
use serde_json::{Map, Value, json};

use crate::redact::{SecretValues, redact_headers, redact_text, redact_url};

/// One thing that happened in a streaming session, in the order it happened.
#[derive(Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Connecting { target: String },
    Connected { details: ConnectionDetails },
    Sent(Message),
    Received(Message),
    Error(SessionError),
    Closed { reason: CloseReason },
}

// A target URL can carry `user:password@`; Debug shows it through `redact_url`. The other
// variants delegate to Debug impls that already hide header values and payload bytes.
impl fmt::Debug for SessionEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connecting { target } => f
                .debug_struct("Connecting")
                .field("target", &redact_url(target, &SecretValues::new()))
                .finish(),
            Self::Connected { details } => f
                .debug_struct("Connected")
                .field("details", details)
                .finish(),
            Self::Sent(message) => f.debug_tuple("Sent").field(message).finish(),
            Self::Received(message) => f.debug_tuple("Received").field(message).finish(),
            Self::Error(error) => f.debug_tuple("Error").field(error).finish(),
            Self::Closed { reason } => f.debug_struct("Closed").field("reason", reason).finish(),
        }
    }
}

/// One message sent or received in a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Milliseconds since the Unix epoch. Absolute, so the log can show it either absolute or
    /// relative to the connect event.
    pub at: u64,
    pub kind: MessageKind,
    pub payload: Payload,
    /// The payload's full size in bytes, also when only a prefix is held in memory.
    pub size: u64,
    pub fields: ProtocolFields,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    Json,
    Binary,
    /// A named protocol event: a Socket.IO emit or an SSE event.
    Event,
}

impl MessageKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
            Self::Binary => "binary",
            Self::Event => "event",
        }
    }
}

/// A message's bytes: held in memory, or spilled to a file by the session store with only
/// a prefix kept for previews.
#[derive(Clone, PartialEq, Eq)]
pub enum Payload {
    Inline(Vec<u8>),
    Spilled {
        path: PathBuf,
        len: u64,
        prefix: Vec<u8>,
    },
}

impl Payload {
    /// The bytes held in memory: the whole payload, or the spilled payload's prefix.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Inline(bytes) => bytes,
            Self::Spilled { prefix, .. } => prefix,
        }
    }
}

// Payloads are message bodies, which are never logged; Debug shows sizes only.
impl fmt::Debug for Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inline(bytes) => f.debug_struct("Inline").field("len", &bytes.len()).finish(),
            Self::Spilled { len, prefix, .. } => f
                .debug_struct("Spilled")
                .field("len", len)
                .field("prefix_len", &prefix.len())
                .finish(),
        }
    }
}

/// The protocol-specific part of a message (REQ-LOG-02). New protocols add a variant; the
/// export tags each with `protocol`, so adding one (MQTT in M6) changes no existing line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolFields {
    None,
    SocketIo {
        event: String,
        ack_id: Option<u64>,
    },
    Grpc {
        message_index: u64,
    },
    Sse {
        event: Option<String>,
        id: Option<String>,
        retry: Option<u64>,
    },
    WebSocket {
        opcode: WebSocketOpcode,
    },
}

/// The WebSocket frame opcodes a message log shows (RFC 6455 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketOpcode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl WebSocketOpcode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Continuation => "continuation",
            Self::Text => "text",
            Self::Binary => "binary",
            Self::Close => "close",
            Self::Ping => "ping",
            Self::Pong => "pong",
        }
    }
}

/// What the handshake established: the headers each side sent, the status and the
/// negotiated subprotocol. Headers keep their order and duplicates.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionDetails {
    pub request_headers: Vec<(String, String)>,
    pub response_status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub protocol: Option<String>,
}

impl ConnectionDetails {
    /// These details with every credential header masked and every known secret removed.
    pub fn redacted(&self, secrets: &SecretValues) -> Self {
        Self {
            request_headers: redact_headers(&self.request_headers, secrets),
            response_status: self.response_status,
            response_headers: redact_headers(&self.response_headers, secrets),
            protocol: self
                .protocol
                .as_deref()
                .map(|protocol| redact_text(protocol, secrets)),
        }
    }
}

// Handshake headers carry `Authorization` and cookies; Debug shows them redacted.
impl fmt::Debug for ConnectionDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted = self.redacted(&SecretValues::new());
        f.debug_struct("ConnectionDetails")
            .field("request_headers", &redacted.request_headers)
            .field("response_status", &redacted.response_status)
            .field("response_headers", &redacted.response_headers)
            .field("protocol", &redacted.protocol)
            .finish()
    }
}

/// A failure the session reports to the user, already phrased for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionError {
    pub message: String,
}

/// Why a session ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// The user closed it, or its driving task was dropped.
    Client,
    /// The server closed it, with the protocol's close code and reason when it sent one.
    Server { code: Option<u16>, reason: String },
    /// The connection was lost without a close handshake.
    Lost,
}

impl SessionEvent {
    /// This event with every credential header and URL password masked and every occurrence
    /// of a known secret removed from text: the form any stored or exported event takes.
    /// Binary payloads are kept as they are; masking text inside arbitrary bytes would
    /// corrupt them.
    pub fn redacted(&self, secrets: &SecretValues) -> Self {
        match self {
            Self::Connecting { target } => Self::Connecting {
                target: redact_url(target, secrets),
            },
            Self::Connected { details } => Self::Connected {
                details: details.redacted(secrets),
            },
            Self::Sent(message) => Self::Sent(message.redacted(secrets)),
            Self::Received(message) => Self::Received(message.redacted(secrets)),
            Self::Error(error) => Self::Error(SessionError {
                message: redact_text(&error.message, secrets),
            }),
            Self::Closed { reason } => Self::Closed {
                reason: match reason {
                    CloseReason::Server { code, reason } => CloseReason::Server {
                        code: *code,
                        reason: redact_text(reason, secrets),
                    },
                    other => other.clone(),
                },
            },
        }
    }
}

impl Message {
    fn redacted(&self, secrets: &SecretValues) -> Self {
        let payload = if self.kind == MessageKind::Binary || secrets.is_empty() {
            self.payload.clone()
        } else {
            match &self.payload {
                Payload::Inline(bytes) => match std::str::from_utf8(bytes) {
                    Ok(text) => Payload::Inline(redact_text(text, secrets).into_bytes()),
                    Err(_) => self.payload.clone(),
                },
                Payload::Spilled { path, len, prefix } => match std::str::from_utf8(prefix) {
                    // The prefix ends mid-payload, so a secret can start inside it and end
                    // past it; cut that tail before masking.
                    Ok(text) => Payload::Spilled {
                        path: path.clone(),
                        len: *len,
                        prefix: redact_text(secrets.straddle_free_prefix(text), secrets)
                            .into_bytes(),
                    },
                    Err(_) => self.payload.clone(),
                },
            }
        };
        Self {
            payload,
            fields: self.fields.clone(),
            ..*self
        }
    }
}

/// One event as one JSON line (no trailing newline), for the session log export.
///
/// Credential headers and URL passwords are always masked; a caller that knows the session's
/// secret values passes the event through [`SessionEvent::redacted`] first. Binary payloads
/// are base64, text payloads stay readable, and a spilled payload carries its in-memory prefix
/// with `"spilled": true` until the export reads the spilled bytes back.
pub fn to_jsonl_line(event: &SessionEvent) -> String {
    let event = event.redacted(&SecretValues::new());
    let value = match &event {
        SessionEvent::Connecting { target } => json!({ "event": "connecting", "target": target }),
        SessionEvent::Connected { details } => json!({
            "event": "connected",
            "details": {
                "request_headers": details.request_headers,
                "response_status": details.response_status,
                "response_headers": details.response_headers,
                "protocol": details.protocol,
            },
        }),
        SessionEvent::Sent(message) => json!({ "event": "sent", "message": message_json(message) }),
        SessionEvent::Received(message) => {
            json!({ "event": "received", "message": message_json(message) })
        }
        SessionEvent::Error(error) => json!({ "event": "error", "message": error.message }),
        SessionEvent::Closed { reason } => {
            json!({ "event": "closed", "reason": close_json(reason) })
        }
    };
    // `Value`'s Display is compact JSON, and JSON strings escape every line break.
    value.to_string()
}

fn message_json(message: &Message) -> Value {
    let mut object = Map::new();
    object.insert("at".to_owned(), json!(message.at));
    object.insert("kind".to_owned(), json!(message.kind.as_str()));
    object.insert("size".to_owned(), json!(message.size));
    object.insert(
        "payload".to_owned(),
        payload_json(message.kind, &message.payload),
    );
    if let Some(fields) = fields_json(&message.fields) {
        object.insert("fields".to_owned(), fields);
    }
    Value::Object(object)
}

fn payload_json(kind: MessageKind, payload: &Payload) -> Value {
    let bytes = payload.bytes();
    let text = match kind {
        MessageKind::Binary => None,
        MessageKind::Text | MessageKind::Json | MessageKind::Event => {
            std::str::from_utf8(bytes).ok()
        }
    };
    let mut object = Map::new();
    match text {
        Some(text) => {
            object.insert("encoding".to_owned(), json!("utf8"));
            object.insert("data".to_owned(), json!(text));
        }
        // Text that is not valid UTF-8 is exported as bytes rather than lossily rewritten.
        None => {
            object.insert("encoding".to_owned(), json!("base64"));
            object.insert(
                "data".to_owned(),
                json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
            );
        }
    }
    if let Payload::Spilled { len, .. } = payload {
        // The spill path is machine-local, so it is not exported.
        object.insert("spilled".to_owned(), json!(true));
        object.insert("len".to_owned(), json!(len));
    }
    Value::Object(object)
}

fn fields_json(fields: &ProtocolFields) -> Option<Value> {
    match fields {
        ProtocolFields::None => None,
        ProtocolFields::SocketIo { event, ack_id } => Some(json!({
            "protocol": "socket_io",
            "event": event,
            "ack_id": ack_id,
        })),
        ProtocolFields::Grpc { message_index } => Some(json!({
            "protocol": "grpc",
            "message_index": message_index,
        })),
        ProtocolFields::Sse { event, id, retry } => Some(json!({
            "protocol": "sse",
            "event": event,
            "id": id,
            "retry": retry,
        })),
        ProtocolFields::WebSocket { opcode } => Some(json!({
            "protocol": "websocket",
            "opcode": opcode.as_str(),
        })),
    }
}

fn close_json(reason: &CloseReason) -> Value {
    match reason {
        CloseReason::Client => json!({ "by": "client" }),
        CloseReason::Server { code, reason } => {
            json!({ "by": "server", "code": code, "reason": reason })
        }
        CloseReason::Lost => json!({ "by": "lost" }),
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
