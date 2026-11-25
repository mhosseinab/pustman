//! Request kinds beyond HTTP: GraphQL, WebSocket, Socket.IO and gRPC (PRD §5.2, §5.4–§5.6).
//!
//! Each kind reuses the HTTP request's key/value rows, auth and settings, and keeps unknown
//! fields in `extra` (REQ-MODEL-02).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ItemId;
use crate::auth::Auth;
use crate::request::KeyValue;
use crate::settings::RequestSettings;

/// A GraphQL request: sent as `POST application/json {query, variables, operationName}`
/// (REQ-GQL-01).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphQlRequest {
    pub id: ItemId,
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    #[serde(default)]
    pub query: String,
    /// The variables editor's text as typed, not parsed JSON, so a half-written document
    /// survives a save.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub variables: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<RequestSettings>,
    /// Unknown fields, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// How a message payload is written and sent (REQ-WS-03).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Text,
    /// Text that is validated as JSON before sending.
    Json,
    /// Bytes, written as Base64 or hex in the payload.
    Binary,
}

/// A message saved by name inside a request so it can be resent with one click (REQ-WS-03).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedMessage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: MessageKind,
    #[serde(default)]
    pub payload: String,
}

/// A raw WebSocket request (REQ-WS-01, REQ-WS-03).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WebSocketRequest {
    pub id: ItemId,
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<KeyValue>,
    /// Offered in `Sec-WebSocket-Protocol`, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subprotocols: Vec<String>,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<RequestSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_messages: Vec<SavedMessage>,
    /// Unknown fields, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// An event emit saved by name inside a Socket.IO request (REQ-SIO-02).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedEmit {
    #[serde(default)]
    pub name: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<SavedMessage>,
    /// Whether the emit asks the server for an acknowledgement.
    #[serde(default)]
    pub acknowledge: bool,
}

/// A Socket.IO request (REQ-SIO-01..03).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SocketIoRequest {
    pub id: ItemId,
    pub name: String,
    pub url: String,
    #[serde(default = "default_socketio_path")]
    pub path: String,
    #[serde(default = "default_socketio_namespace")]
    pub namespace: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    /// The handshake `auth` payload as typed (JSON text), kept raw like GraphQL variables.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handshake_timeout_ms: Option<u64>,
    /// Event names to subscribe to when `all_events` is off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listen: Vec<String>,
    #[serde(default = "crate::enabled_by_default")]
    pub all_events: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_emits: Vec<SavedEmit>,
    /// Unknown fields, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

fn default_socketio_path() -> String {
    "/socket.io".to_string()
}

fn default_socketio_namespace() -> String {
    "/".to_string()
}

/// Where a gRPC request's service definitions come from (REQ-GRPC-01).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DescriptorSource {
    /// Ask the server (`grpc.reflection.v1`, falling back to `v1alpha`).
    #[default]
    Reflection,
    /// `.proto` files, usually under the workspace's `protos/` folder.
    ProtoFiles {
        #[serde(default)]
        files: Vec<String>,
        #[serde(default)]
        import_paths: Vec<String>,
    },
    /// A compiled `FileDescriptorSet` file.
    DescriptorSet { path: String },
}

/// A gRPC request (REQ-GRPC-01..04).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrpcRequest {
    pub id: ItemId,
    pub name: String,
    /// `host:port`, or a URL with a scheme.
    pub target: String,
    /// Fully qualified service name (`helloworld.Greeter`).
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub method: String,
    /// The request message as proto3 JSON text, kept raw so an invalid draft survives a save.
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
    /// TLS is on unless the request opts into plaintext HTTP/2 (h2c) for a local server.
    #[serde(default = "crate::enabled_by_default")]
    pub tls: bool,
    #[serde(default)]
    pub descriptor_source: DescriptorSource,
    /// Unknown fields, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
#[path = "protocol_request_tests.rs"]
mod tests;
