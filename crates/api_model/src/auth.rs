//! Authentication kinds and their inheritance (PRD REQ-MODEL-01, REQ-HTTP-06).

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::auth_extended::ExtendedKind;
use crate::postman::{ImportReport, import_auth};
use crate::{
    AwsSigV4Auth, DigestAuth, JwtBearerAuth, OAuth2Auth, extra_to_map, map_to_extra, object_tag,
    serialize_tagged, take_field,
};

/// Where an API key is placed on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyLocation {
    Header,
    Query,
}

/// How a request authenticates (PRD §4).
///
/// `Inherit` walks up to the parent folder and then the collection; `None` is an explicit
/// "no auth" that stops the walk; a kind we cannot model is kept raw in `Unsupported` so an
/// export round-trips what an import brought in (REQ-MODEL-02).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Auth {
    /// Take the parent folder's or collection's auth.
    #[default]
    Inherit,
    /// Send no credentials even when a parent has some.
    None,
    Basic {
        username: Option<String>,
        password: Option<String>,
        extra: BTreeMap<String, serde_json::Value>,
    },
    Bearer {
        token: Option<String>,
        extra: BTreeMap<String, serde_json::Value>,
    },
    ApiKey {
        key: Option<String>,
        value: Option<String>,
        location: Option<ApiKeyLocation>,
        extra: BTreeMap<String, serde_json::Value>,
    },
    Digest(DigestAuth),
    OAuth2(OAuth2Auth),
    AwsSigV4(AwsSigV4Auth),
    JwtBearer(JwtBearerAuth),
    /// An auth kind we do not model (Hawk, NTLM, OAuth 1.0, ASAP, EdgeGrid, …), preserved
    /// verbatim.
    Unsupported(serde_json::Value),
}

impl Auth {
    /// Whether this level passes the decision up to its parent.
    pub fn is_inherit(&self) -> bool {
        matches!(self, Auth::Inherit)
    }

    fn kind(&self) -> &'static str {
        match self {
            Auth::Inherit => "inherit",
            Auth::None => "none",
            Auth::Basic { .. } => "basic",
            Auth::Bearer { .. } => "bearer",
            Auth::ApiKey { .. } => "api_key",
            Auth::Digest(_) => "digest",
            Auth::OAuth2(_) => "oauth2",
            Auth::AwsSigV4(_) => "aws_sigv4",
            Auth::JwtBearer(_) => "jwt_bearer",
            Auth::Unsupported(_) => "unsupported",
        }
    }
}

/// The auth a send ends up using once inheritance resolved.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectiveAuth {
    None,
    Basic {
        username: Option<String>,
        password: Option<String>,
    },
    Bearer {
        token: Option<String>,
    },
    ApiKey {
        key: Option<String>,
        value: Option<String>,
        location: Option<ApiKeyLocation>,
    },
    Unsupported(serde_json::Value),
}

/// Which level of the collection tree decided the effective auth. The Auth tab shows this
/// (REQ-MODEL-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthSource {
    Request,
    Folder,
    Collection,
    /// Nothing at any level set auth: no credentials are sent.
    Default,
}

/// Resolve the effective auth by walking `chain` from the request up to the collection.
///
/// `chain[0]` is the request's auth, the last entry is the collection's, and anything between
/// is a folder. `Auth::Inherit` continues the walk, `Auth::None` stops it with no auth, and
/// the first concrete kind wins.
pub fn effective_auth(chain: &[&Auth]) -> (EffectiveAuth, AuthSource) {
    let last = chain.len().saturating_sub(1);
    for (index, auth) in chain.iter().enumerate() {
        let source = if index == 0 {
            AuthSource::Request
        } else if index == last {
            AuthSource::Collection
        } else {
            AuthSource::Folder
        };
        match auth {
            Auth::Inherit => continue,
            Auth::None => return (EffectiveAuth::None, source),
            Auth::Basic {
                username, password, ..
            } => {
                return (
                    EffectiveAuth::Basic {
                        username: username.clone(),
                        password: password.clone(),
                    },
                    source,
                );
            }
            Auth::Bearer { token, .. } => {
                return (
                    EffectiveAuth::Bearer {
                        token: token.clone(),
                    },
                    source,
                );
            }
            Auth::ApiKey {
                key,
                value,
                location,
                ..
            } => {
                return (
                    EffectiveAuth::ApiKey {
                        key: key.clone(),
                        value: value.clone(),
                        location: *location,
                    },
                    source,
                );
            }
            // `Decision:` nothing signs these kinds yet, so the engine reports them exactly
            // as it reports a raw kind — by name only, which keeps every credential they
            // hold out of the effective auth.
            Auth::Digest(_) | Auth::OAuth2(_) | Auth::AwsSigV4(_) | Auth::JwtBearer(_) => {
                let kind = serde_json::json!({ "type": auth.kind() });
                return (EffectiveAuth::Unsupported(kind), source);
            }
            Auth::Unsupported(raw) => return (EffectiveAuth::Unsupported(raw.clone()), source),
        }
    }
    (EffectiveAuth::None, AuthSource::Default)
}

impl Serialize for Auth {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serde_json::Map::new();
        match self {
            Auth::Inherit | Auth::None => {}
            Auth::Basic {
                username,
                password,
                extra,
            } => {
                fields = extra_to_map(extra);
                insert_optional::<S>(&mut fields, "username", username)?;
                insert_optional::<S>(&mut fields, "password", password)?;
            }
            Auth::Bearer { token, extra } => {
                fields = extra_to_map(extra);
                insert_optional::<S>(&mut fields, "token", token)?;
            }
            Auth::ApiKey {
                key,
                value,
                location,
                extra,
            } => {
                fields = extra_to_map(extra);
                insert_optional::<S>(&mut fields, "key", key)?;
                insert_optional::<S>(&mut fields, "value", value)?;
                insert_optional::<S>(&mut fields, "location", location)?;
            }
            Auth::Digest(digest) => fields = object_fields::<S, _>(digest)?,
            Auth::OAuth2(oauth2) => fields = object_fields::<S, _>(oauth2)?,
            Auth::AwsSigV4(aws) => fields = object_fields::<S, _>(aws)?,
            Auth::JwtBearer(jwt) => fields = object_fields::<S, _>(jwt)?,
            Auth::Unsupported(raw) => return raw.serialize(serializer),
        }
        serialize_tagged(serializer, "type", self.kind(), fields)
    }
}

impl<'de> Deserialize<'de> for Auth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(parse_auth(serde_json::Value::deserialize(deserializer)?))
    }
}

/// Decode one auth object. Anything unrecognized is kept raw so export preserves it.
fn parse_auth(value: serde_json::Value) -> Auth {
    let Some(tag) = object_tag(&value, "type") else {
        return Auth::Unsupported(value);
    };
    let Some(object) = value.as_object() else {
        return Auth::Unsupported(value);
    };

    // `inherit` and `none` carry no fields, so an object holding anything else is not that shape.
    let is_bare = object.len() == 1;
    match tag {
        "inherit" if is_bare => return Auth::Inherit,
        "none" if is_bare => return Auth::None,
        _ => {}
    }

    // A workspace saved before these kinds were modelled kept an imported Postman auth raw,
    // its fields still in the Postman group (`"digest": [...]`): map it as an import would.
    if matches!(tag, "digest" | "oauth2" | "awsv4" | "jwt") && object.contains_key(tag) {
        return import_auth(&value, "auth", &mut ImportReport::default());
    }

    let mut fields = object.clone();
    fields.remove("type");
    let parsed = match tag {
        "basic" => read_basic(fields),
        "bearer" => read_bearer(fields),
        "api_key" => read_api_key(fields),
        "digest" => read_kind(fields).map(Auth::Digest),
        "oauth2" => read_kind(fields).map(Auth::OAuth2),
        "aws_sigv4" => read_kind(fields).map(Auth::AwsSigV4),
        "jwt_bearer" => read_kind(fields).map(Auth::JwtBearer),
        _ => None,
    };
    parsed.unwrap_or(Auth::Unsupported(value))
}

fn read_basic(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Auth> {
    let username = take_field(&mut fields, "username", None).ok()?;
    let password = take_field(&mut fields, "password", None).ok()?;
    Some(Auth::Basic {
        username,
        password,
        extra: map_to_extra(fields),
    })
}

fn read_bearer(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Auth> {
    let token = take_field(&mut fields, "token", None).ok()?;
    Some(Auth::Bearer {
        token,
        extra: map_to_extra(fields),
    })
}

fn read_api_key(mut fields: serde_json::Map<String, serde_json::Value>) -> Option<Auth> {
    let key = take_field(&mut fields, "key", None).ok()?;
    let value = take_field(&mut fields, "value", None).ok()?;
    let location = take_field(&mut fields, "location", None).ok()?;
    Some(Auth::ApiKey {
        key,
        value,
        location,
        extra: map_to_extra(fields),
    })
}

/// Decode one of the extended kinds; a field of the wrong shape keeps the whole auth raw.
fn read_kind<T: serde::de::DeserializeOwned>(
    fields: serde_json::Map<String, serde_json::Value>,
) -> Option<T> {
    serde_json::from_value(serde_json::Value::Object(fields)).ok()
}

/// An extended kind's fields as the JSON object [`serialize_tagged`] writes under its tag:
/// the typed fields, then each `extra` entry whose key names none of them.
fn object_fields<S, K>(kind: &K) -> Result<serde_json::Map<String, serde_json::Value>, S::Error>
where
    S: Serializer,
    K: ExtendedKind,
{
    let mut fields = match serde_json::to_value(kind).map_err(serde::ser::Error::custom)? {
        serde_json::Value::Object(fields) => fields,
        // Never echo the value: it holds the kind's credentials.
        _ => {
            return Err(serde::ser::Error::custom(
                "an auth kind did not serialize as a JSON object",
            ));
        }
    };
    for (key, value) in kind.extra() {
        if !K::FIELDS.contains(&key.as_str()) {
            fields.insert(key.clone(), value.clone());
        }
    }
    Ok(fields)
}

fn insert_optional<S>(
    fields: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
    value: &Option<impl Serialize>,
) -> Result<(), S::Error>
where
    S: Serializer,
{
    if let Some(value) = value {
        fields.insert(
            name.to_string(),
            serde_json::to_value(value).map_err(serde::ser::Error::custom)?,
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
