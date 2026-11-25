//! Plain serde types for the Pustman data model (PRD §4): identities, collections, folders,
//! requests, auth, variables, environments, request settings and saved examples.
//!
//! This crate is a domain crate: no IO, no GPUI. Every item serializes with unknown fields
//! preserved verbatim in `extra` (REQ-MODEL-02) and unknown request/auth/body kinds are kept
//! as `Unsupported(serde_json::Value)` so an export never drops what an import brought in.

mod auth;
mod collection;
mod curl;
mod environment;
mod example;
mod network;
mod postman;
mod protocol_request;
mod redact;
mod request;
mod session;
mod settings;
mod variable;

pub use auth::{ApiKeyLocation, Auth, AuthSource, EffectiveAuth, effective_auth};
pub use collection::{Child, Collection, Folder, Workspace};
pub use curl::{CurlError, parse_curl, to_curl};
pub use environment::Environment;
pub use example::{Example, ExampleResponse};
pub use network::{
    ClientCertificate, ClientCertificateKind, NetworkSettings, ProxyConfig, ProxyMode, ProxyScheme,
    SecretRef,
};
pub use postman::{
    ExportReport, ImportError, ImportLimits, ImportProblem, ImportReport, SecretValuesToStore,
    export_collection, import_collection, import_environment, import_globals, string_field,
};
pub use protocol_request::{
    DescriptorSource, GraphQlRequest, GrpcRequest, MessageKind, SavedEmit, SavedMessage,
    SocketIoRequest, WebSocketRequest,
};
pub use redact::{MASK, SecretValues, redact_headers, redact_text, redact_url};
pub use request::{
    Body, FormField, HttpRequest, KeyValue, Method, MethodError, RawLanguage, Request,
};
pub use session::{
    CloseReason, ConnectionDetails, Message, Payload, ProtocolFields, SessionError, SessionEvent,
    SessionMessageKind, WebSocketOpcode, to_jsonl_line,
};
pub use settings::{HttpVersion, RequestSettings};
pub use variable::{Variable, VariableKind};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::str::FromStr;

/// Stable identity of one collection item: a collection, folder or request.
///
/// Serializes as its canonical 26-character ULID string so saved workspaces stay
/// diffable and ids remain readable in JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(ulid::Ulid);

impl ItemId {
    /// Mint a new random identifier.
    pub fn new() -> Self {
        Self(ulid::Ulid::generate())
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl FromStr for ItemId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ulid::Ulid::from_str(s).map(Self)
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ItemId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ItemId::from_str(&text).map_err(serde::de::Error::custom)
    }
}

/// Default for row flags: a row the user just added is enabled.
pub(crate) fn enabled_by_default() -> bool {
    true
}

/// Serialize `fields` as one JSON object whose first key is `tag_name`, so a tagged model
/// enum (request, body, auth) always shows its discriminator before its payload.
pub(crate) fn serialize_tagged<S>(
    serializer: S,
    tag_name: &str,
    tag: &str,
    fields: serde_json::Map<String, serde_json::Value>,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;

    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry(tag_name, &tag)?;
    for (key, value) in &fields {
        if key != tag_name {
            map.serialize_entry(key, value)?;
        }
    }
    map.end()
}

/// The discriminator string of a JSON object, if it carries one.
pub(crate) fn object_tag<'a>(value: &'a serde_json::Value, tag_name: &str) -> Option<&'a str> {
    value
        .as_object()
        .and_then(|object| object.get(tag_name))
        .and_then(|tag| tag.as_str())
}

/// Take `name` out of a JSON object and decode it, falling back to `fallback` when the key is
/// absent. Unknown keys are left in place so callers can hand them to `extra`.
pub(crate) fn take_field<T>(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    fallback: T,
) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned,
{
    match fields.remove(name) {
        Some(value) => serde_json::from_value(value),
        None => Ok(fallback),
    }
}

/// The fields left after a known shape was decoded: the sorted `extra` map every model type
/// carries (REQ-MODEL-02).
pub(crate) fn map_to_extra(
    fields: serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    fields.into_iter().collect()
}

/// `extra` on its way back out as a JSON object.
pub(crate) fn extra_to_map(
    extra: &BTreeMap<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    extra
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
#[path = "api_model_tests.rs"]
mod tests;
