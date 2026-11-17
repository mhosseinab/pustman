//! Authentication kinds and their inheritance (PRD REQ-MODEL-01, REQ-HTTP-06).

use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{extra_to_map, map_to_extra, object_tag, serialize_tagged, take_field};

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
    /// An auth kind we do not model (Hawk, ASAP, EdgeGrid, Digest, …), preserved verbatim.
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

    let mut fields = object.clone();
    fields.remove("type");
    let parsed = match tag {
        "basic" => read_basic(fields),
        "bearer" => read_bearer(fields),
        "api_key" => read_api_key(fields),
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
