//! HTTP requests: method, URL, key/value tables, body, auth, settings and examples
//! (PRD §4, REQ-HTTP-01..04).

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::str::FromStr;

use crate::auth::Auth;
use crate::example::Example;
use crate::protocol_request::{GraphQlRequest, GrpcRequest, SocketIoRequest, WebSocketRequest};
use crate::settings::RequestSettings;
use crate::{ItemId, extra_to_map, map_to_extra, object_tag, serialize_tagged, take_field};

/// One row of a key/value table: query params, path variables or headers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "crate::enabled_by_default")]
    pub enabled: bool,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One `form-data` row: a text or file field with an optional per-field content type
/// (REQ-HTTP-04, REQ-HTTP-05).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub key: String,
    /// The text payload, when this is a text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// A file reference: relative to the workspace root when inside it, absolute otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "crate::enabled_by_default")]
    pub enabled: bool,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A syntax language a raw body is edited and highlighted in (REQ-HTTP-04).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RawLanguage {
    #[serde(rename = "text")]
    #[default]
    Text,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "xml")]
    Xml,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "javascript")]
    JavaScript,
}

/// A method token: either one of the standard verbs or a custom token (REQ-HTTP-01).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Trace,
    Connect,
    /// A custom method, validated as an RFC 9110 `tchar` token and kept as written.
    Custom(String),
}

/// A method token that is empty or contains a byte outside RFC 9110 `tchar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodError;

impl Display for MethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("method must be a non-empty RFC 9110 token")
    }
}

impl std::error::Error for MethodError {}

impl Method {
    /// Parse a method token. The nine standard verbs are case-sensitive (RFC 7231 §4.1), so
    /// anything else is a custom token and is preserved exactly as written.
    pub fn from_token(text: &str) -> Result<Method, MethodError> {
        match text {
            "GET" => return Ok(Method::Get),
            "POST" => return Ok(Method::Post),
            "PUT" => return Ok(Method::Put),
            "PATCH" => return Ok(Method::Patch),
            "DELETE" => return Ok(Method::Delete),
            "HEAD" => return Ok(Method::Head),
            "OPTIONS" => return Ok(Method::Options),
            "TRACE" => return Ok(Method::Trace),
            "CONNECT" => return Ok(Method::Connect),
            _ => {}
        }
        if is_token(text) {
            Ok(Method::Custom(text.to_string()))
        } else {
            Err(MethodError)
        }
    }

    /// The method exactly as it goes on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Trace => "TRACE",
            Method::Connect => "CONNECT",
            Method::Custom(token) => token,
        }
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Method {
    type Err = MethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Method::from_token(s)
    }
}

impl Serialize for Method {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Method {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Method::from_token(&text).map_err(serde::de::Error::custom)
    }
}

fn is_token(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(is_tchar)
}

fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// A request body (REQ-HTTP-04). A mode we do not model is kept raw in `Unsupported` so an
/// export round-trips what an import brought in (REQ-MODEL-02).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Body {
    /// No body at all: the field is left out of the file.
    #[default]
    None,
    FormData {
        fields: Vec<FormField>,
        extra: BTreeMap<String, serde_json::Value>,
    },
    UrlEncoded {
        fields: Vec<KeyValue>,
        extra: BTreeMap<String, serde_json::Value>,
    },
    Raw {
        language: RawLanguage,
        text: String,
        extra: BTreeMap<String, serde_json::Value>,
    },
    Binary {
        file: String,
        extra: BTreeMap<String, serde_json::Value>,
    },
    /// A body mode we do not model (GraphQL, …), preserved verbatim.
    Unsupported(serde_json::Value),
}

impl Body {
    /// Whether this request sends no body.
    pub fn is_none(&self) -> bool {
        matches!(self, Body::None)
    }

    fn mode(&self) -> &'static str {
        match self {
            Body::None => "none",
            Body::FormData { .. } => "form_data",
            Body::UrlEncoded { .. } => "url_encoded",
            Body::Raw { .. } => "raw",
            Body::Binary { .. } => "binary",
            Body::Unsupported(_) => "unsupported",
        }
    }
}

/// One HTTP request, the only request kind v1 models (plan D7: later milestones add
/// GraphQL, WebSocket, Socket.IO, gRPC and MQTT variants additively).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub id: ItemId,
    pub name: String,
    pub method: Method,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_variables: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "Body::is_none")]
    pub body: Body,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<RequestSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// Unknown fields from imports (scripts, protocol profiles), preserved verbatim
    /// (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A collection item that can send something (PRD §4).
// `Http` is the largest variant because a request carries its tables inline; model values
// are cloned per edit rather than per frame, so boxing would cost more in call sites than it
// saves here.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Http(HttpRequest),
    GraphQl(GraphQlRequest),
    WebSocket(WebSocketRequest),
    SocketIo(SocketIoRequest),
    Grpc(GrpcRequest),
    /// A request kind we do not model yet (MQTT, …), or a known kind whose fields do not
    /// decode, preserved verbatim so an export never drops it.
    Unsupported(serde_json::Value),
}

impl Request {
    /// The `type` tag the request is saved under; for a kind we do not model, the tag its
    /// raw JSON carries (`"request"` when it carries none).
    pub fn type_tag(&self) -> &str {
        match self {
            Request::Http(_) => "http",
            Request::GraphQl(_) => "graphql",
            Request::WebSocket(_) => "websocket",
            Request::SocketIo(_) => "socketio",
            Request::Grpc(_) => "grpc",
            Request::Unsupported(raw) => object_tag(raw, "type").unwrap_or("request"),
        }
    }

    /// The request's identity; for a raw kind, the `id` its JSON carries when that parses.
    pub fn id(&self) -> Option<ItemId> {
        match self {
            Request::Http(request) => Some(request.id),
            Request::GraphQl(request) => Some(request.id),
            Request::WebSocket(request) => Some(request.id),
            Request::SocketIo(request) => Some(request.id),
            Request::Grpc(request) => Some(request.id),
            Request::Unsupported(raw) => raw
                .get("id")
                .and_then(serde_json::Value::as_str)
                .and_then(|id| id.parse().ok()),
        }
    }

    /// Give the request a new identity. A raw kind keeps its shape and swaps only the `id`
    /// key; a raw value that is not an object has nowhere to hold one and is left as is.
    pub fn set_id(&mut self, id: ItemId) {
        match self {
            Request::Http(request) => request.id = id,
            Request::GraphQl(request) => request.id = id,
            Request::WebSocket(request) => request.id = id,
            Request::SocketIo(request) => request.id = id,
            Request::Grpc(request) => request.id = id,
            Request::Unsupported(raw) => {
                if let Some(object) = raw.as_object_mut() {
                    object.insert("id".to_string(), serde_json::Value::from(id.to_string()));
                }
            }
        }
    }

    /// The display name; for a raw kind, its `name` string, or empty when it has none.
    pub fn name(&self) -> &str {
        match self {
            Request::Http(request) => &request.name,
            Request::GraphQl(request) => &request.name,
            Request::WebSocket(request) => &request.name,
            Request::SocketIo(request) => &request.name,
            Request::Grpc(request) => &request.name,
            Request::Unsupported(raw) => raw
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        }
    }

    /// Rename the request. A raw kind that is not an object has nowhere to hold a name and
    /// is left as is.
    pub fn set_name(&mut self, name: &str) {
        match self {
            Request::Http(request) => request.name = name.to_string(),
            Request::GraphQl(request) => request.name = name.to_string(),
            Request::WebSocket(request) => request.name = name.to_string(),
            Request::SocketIo(request) => request.name = name.to_string(),
            Request::Grpc(request) => request.name = name.to_string(),
            Request::Unsupported(raw) => {
                if let Some(object) = raw.as_object_mut() {
                    object.insert("name".to_string(), serde_json::Value::from(name));
                }
            }
        }
    }
}

impl Serialize for Request {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Request::Http(request) => serialize_kind(serializer, "http", request),
            Request::GraphQl(request) => serialize_kind(serializer, "graphql", request),
            Request::WebSocket(request) => serialize_kind(serializer, "websocket", request),
            Request::SocketIo(request) => serialize_kind(serializer, "socketio", request),
            Request::Grpc(request) => serialize_kind(serializer, "grpc", request),
            Request::Unsupported(raw) => raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(parse_request(serde_json::Value::deserialize(deserializer)?))
    }
}

/// Decode one request object. A known `type` with decodable fields becomes typed; every
/// other shape is kept raw so export preserves it.
fn parse_request(value: serde_json::Value) -> Request {
    let parsed = match object_tag(&value, "type") {
        Some("http") => decode_kind(&value).map(Request::Http),
        Some("graphql") => decode_kind(&value).map(Request::GraphQl),
        Some("websocket") => decode_kind(&value).map(Request::WebSocket),
        Some("socketio") => decode_kind(&value).map(Request::SocketIo),
        Some("grpc") => decode_kind(&value).map(Request::Grpc),
        _ => None,
    };
    parsed.unwrap_or(Request::Unsupported(value))
}

/// Decode a tagged request object's fields (everything but `type`) as one request kind.
fn decode_kind<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Option<T> {
    let mut fields = value.as_object()?.clone();
    fields.remove("type");
    serde_json::from_value(serde_json::Value::Object(fields)).ok()
}

/// Serialize one request kind as a JSON object with its `type` tag first.
fn serialize_kind<S: Serializer, T: Serialize>(
    serializer: S,
    tag: &str,
    request: &T,
) -> Result<S::Ok, S::Error> {
    match serde_json::to_value(request) {
        Ok(serde_json::Value::Object(fields)) => serialize_tagged(serializer, "type", tag, fields),
        Ok(_) => Err(serde::ser::Error::custom(
            "a request must serialize to an object",
        )),
        Err(error) => Err(serde::ser::Error::custom(error)),
    }
}

impl Serialize for Body {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;

        let mut fields = serde_json::Map::new();
        match self {
            Body::None => {}
            Body::FormData {
                fields: rows,
                extra,
            } => {
                fields = extra_to_map(extra);
                fields.insert(
                    "fields".to_string(),
                    serde_json::to_value(rows).map_err(Error::custom)?,
                );
            }
            Body::UrlEncoded {
                fields: rows,
                extra,
            } => {
                fields = extra_to_map(extra);
                fields.insert(
                    "fields".to_string(),
                    serde_json::to_value(rows).map_err(Error::custom)?,
                );
            }
            Body::Raw {
                language,
                text,
                extra,
            } => {
                fields = extra_to_map(extra);
                fields.insert(
                    "language".to_string(),
                    serde_json::to_value(language).map_err(Error::custom)?,
                );
                fields.insert(
                    "text".to_string(),
                    serde_json::to_value(text).map_err(Error::custom)?,
                );
            }
            Body::Binary { file, extra } => {
                fields = extra_to_map(extra);
                fields.insert(
                    "file".to_string(),
                    serde_json::to_value(file).map_err(Error::custom)?,
                );
            }
            Body::Unsupported(raw) => return raw.serialize(serializer),
        }
        serialize_tagged(serializer, "mode", self.mode(), fields)
    }
}

impl<'de> Deserialize<'de> for Body {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(parse_body(serde_json::Value::deserialize(deserializer)?))
    }
}

/// Decode one body object. `null` and a missing field mean "no body"; a mode we do not model
/// is kept raw so export preserves it.
fn parse_body(value: serde_json::Value) -> Body {
    if value.is_null() {
        return Body::None;
    }
    let Some(object) = value.as_object() else {
        return Body::Unsupported(value);
    };
    let Some(mode) = object_tag(&value, "mode") else {
        return Body::Unsupported(value);
    };

    // `none` carries no fields, so an object holding anything else is not that shape.
    if mode == "none" {
        return if object.len() == 1 {
            Body::None
        } else {
            Body::Unsupported(value)
        };
    }

    let mut fields = object.clone();
    fields.remove("mode");
    let parsed = match mode {
        "form_data" => read_form_data(fields),
        "url_encoded" => read_url_encoded(fields),
        "raw" => read_raw(fields),
        "binary" => read_binary(fields),
        _ => None,
    };
    parsed.unwrap_or(Body::Unsupported(value))
}

fn read_form_data(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Body> {
    let rows = take_field(&mut fields, "fields", Vec::new()).ok()?;
    Some(Body::FormData {
        fields: rows,
        extra: map_to_extra(fields),
    })
}

fn read_url_encoded(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Body> {
    let rows = take_field(&mut fields, "fields", Vec::new()).ok()?;
    Some(Body::UrlEncoded {
        fields: rows,
        extra: map_to_extra(fields),
    })
}

fn read_raw(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Body> {
    let language = take_field(&mut fields, "language", RawLanguage::default()).ok()?;
    let text = take_field(&mut fields, "text", String::new()).ok()?;
    Some(Body::Raw {
        language,
        text,
        extra: map_to_extra(fields),
    })
}

fn read_binary(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Body> {
    let file = take_field(&mut fields, "file", String::new()).ok()?;
    Some(Body::Binary {
        file,
        extra: map_to_extra(fields),
    })
}

#[cfg(test)]
#[path = "method_error_tests.rs"]
mod method_error_tests;

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
