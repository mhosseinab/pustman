//! Applying the effective auth to a request before it goes on the wire (PRD REQ-MODEL-01,
//! REQ-HTTP-06).
//!
//! Pure: no IO and no logging, because every value this adds is a credential
//! (AGENTS.md › Security).

use api_model::{ApiKeyLocation, Auth, EffectiveAuth, effective_auth};
use base64::Engine;

use crate::request::{Header, ResolvedRequest};

/// What [`apply_auth`] did to the request, so the caller can tell the user.
///
/// S13c's `prepare` turns [`AppliedAuth::Unsupported`] into a user-visible "auth type
/// not supported yet" outcome instead of sending silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedAuth {
    /// The effective auth is explicit no-auth: no credentials were added.
    None,
    /// Basic, Bearer or API key credentials were added to the request.
    Applied,
    /// Nothing was applied: the auth kind is outside the M1 subset. `kind` is only the
    /// raw value's `"type"` string — the raw object can hold credentials, so it is
    /// never carried, logged or reported.
    Unsupported { kind: String },
    /// Nothing was applied: the configured auth needs a field that resolved to empty.
    /// `kind` is the auth kind and `missing` names the empty field — both static
    /// strings, never a field value. S13c's `prepare` surfaces this to the user like
    /// [`AppliedAuth::Unsupported`]: no silent send.
    Incomplete {
        kind: &'static str,
        missing: &'static str,
    },
}

/// Apply the auth a send ends up using to `resolved`'s headers and query, reporting
/// what happened in [`AppliedAuth`].
///
/// `own` is the request's auth; `ancestors` are its parents' auths ordered nearest
/// first (folder → … → collection). [`Auth::Inherit`] walks that chain and the first
/// concrete kind wins (`api_model::effective_auth`), [`Auth::None`] stops it with no
/// credentials, and a kind outside the M1 subset adds nothing and comes back as
/// [`AppliedAuth::Unsupported`] — the caller (S13c's `prepare`) turns that into a
/// user-visible "auth type not supported yet" outcome rather than a silent send.
/// Field values go through `resolve` so `{{variables}}` work before anything reaches
/// the wire.
pub fn apply_auth(
    resolved: &mut ResolvedRequest,
    own: &Auth,
    ancestors: &[&Auth],
    resolve: impl Fn(&str) -> String,
) -> AppliedAuth {
    let chain: Vec<&Auth> = std::iter::once(own)
        .chain(ancestors.iter().copied())
        .collect();
    let (effective, _source) = effective_auth(&chain);
    match effective {
        // Explicit "no auth": the request goes out as the user built it.
        EffectiveAuth::None => AppliedAuth::None,
        // A kind this milestone does not model: nothing is applied, and only the kind
        // name is reported — the raw object can hold credentials.
        EffectiveAuth::Unsupported(raw) => AppliedAuth::Unsupported {
            kind: raw
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        },
        EffectiveAuth::Basic { username, password } => {
            let username = resolved_field(&resolve, username);
            let password = resolved_field(&resolve, password);
            // RFC 7617 allows an empty username or password — only both empty means
            // `Basic Og==` would carry no usable credential.
            if let Some(incomplete) =
                incomplete_when_empty("basic", "username", &[&username, &password])
            {
                return incomplete;
            }
            let credentials = format!("{username}:{password}");
            set_header(
                resolved,
                "Authorization",
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes())
                ),
            );
            AppliedAuth::Applied
        }
        EffectiveAuth::Bearer { token } => {
            let token = resolved_field(&resolve, token);
            if let Some(incomplete) = incomplete_when_empty("bearer", "token", &[&token]) {
                return incomplete;
            }
            set_header(resolved, "Authorization", format!("Bearer {token}"));
            AppliedAuth::Applied
        }
        EffectiveAuth::ApiKey {
            key,
            value,
            location,
        } => {
            let name = resolved_field(&resolve, key);
            // Nothing to place the value under: a nameless header or query pair is
            // unreadable on the other side.
            if let Some(incomplete) = incomplete_when_empty("api key", "key name", &[&name]) {
                return incomplete;
            }
            let value = resolved_field(&resolve, value);
            // An empty header or `X-Key=` carries no credential either.
            if let Some(incomplete) = incomplete_when_empty("api key", "key value", &[&value]) {
                return incomplete;
            }
            // A missing placement is the header, Postman's default for an API key.
            match location.unwrap_or(ApiKeyLocation::Header) {
                ApiKeyLocation::Header => set_header(resolved, &name, value),
                ApiKeyLocation::Query => {
                    resolved.url.query_pairs_mut().append_pair(&name, &value);
                }
            }
            AppliedAuth::Applied
        }
    }
}

/// One auth field, resolved: an absent field is an empty value, and only present text
/// goes through `resolve`.
fn resolved_field(resolve: &impl Fn(&str) -> String, field: Option<String>) -> String {
    field.as_deref().map(resolve).unwrap_or_default()
}

/// The shared "a configured field resolved to empty" check behind every
/// [`AppliedAuth::Incomplete`]: when every credential field the auth needs (`fields`)
/// resolved to empty, no usable credential reaches the wire, so nothing is applied
/// and `missing` names the reported field — never a field value.
fn incomplete_when_empty(
    kind: &'static str,
    missing: &'static str,
    fields: &[&str],
) -> Option<AppliedAuth> {
    fields
        .iter()
        .all(|field| field.is_empty())
        .then_some(AppliedAuth::Incomplete { kind, missing })
}

/// Put `name: value` in the headers, replacing what the request carried under that name:
/// one field per name, since a second `Authorization` is not valid HTTP. The position of
/// the field the request already had is kept.
fn set_header(resolved: &mut ResolvedRequest, name: &str, value: String) {
    let mut placed = false;
    resolved.headers.retain_mut(|header| {
        if !header.name.eq_ignore_ascii_case(name) {
            return true;
        }
        if placed {
            return false;
        }
        header.name = name.to_string();
        header.value.clone_from(&value);
        placed = true;
        true
    });
    if !placed {
        resolved.headers.push(Header::new(name, value));
    }
}

#[cfg(test)]
#[path = "request_auth_tests.rs"]
mod tests;
