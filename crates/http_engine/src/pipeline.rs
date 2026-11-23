//! Resolving every request field through `{{variables}}` before a send (PRD §7,
//! REQ-VAR-01/02/07).
//!
//! [`resolve_request`] is the whole pipeline: URL, query params, headers and
//! raw/urlencoded body text go through [`variables::resolve`], and the names nothing
//! resolves come back as a [`Warning`] while their placeholders stay literal, so the
//! send can still run (REQ-VAR-07). One [`DynamicValues`] is one send: the same
//! `{{$guid}}` carries one value through every field (REQ-VAR-05).
//!
//! This is a pure step — no IO, and nothing it produces is logged. Auth (S13b), secret
//! lookups (S13c) and body encoding (S14a) are later pipeline steps.

use std::path::PathBuf;
use std::time::Duration;

use api_model::{Body, HttpRequest, KeyValue, RequestSettings};
use url::{Url, form_urlencoded};
use variables::{DynamicValues, ResolveError, ScopeStack, resolve};

use crate::error::TransportError;
use crate::request::{
    Header, MultipartField, MultipartValue, RequestBody, ResolvedRequest, SendSettings,
};
use crate::request_auth::AppliedAuth;

/// What a send should tell the user before it runs (REQ-VAR-07, REQ-HTTP-10).
///
/// A warning never stops a send and carries names only, never values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Names nothing resolved, in first-appearance order across the request and without
    /// duplicates. Each stays literal in the resolved request, so sending anyway remains
    /// possible (REQ-VAR-07).
    UnresolvedVariables(Vec<String>),
    /// The request turns TLS certificate verification off; the UI must flag it visibly
    /// (REQ-HTTP-10).
    TlsVerifyDisabled,
    /// The configured auth added no credential: a field it needs resolved to empty
    /// (`AppliedAuth::Incomplete`). The send still runs, without credentials, and the
    /// user is told so (REQ-HTTP-06). Both strings are static names, never field values.
    AuthIncomplete {
        kind: &'static str,
        missing: &'static str,
    },
    /// The effective auth is a kind outside the supported subset
    /// (`AppliedAuth::Unsupported`): nothing was applied and the send runs without
    /// credentials (REQ-HTTP-06). `kind` is only the auth's `"type"` name.
    AuthUnsupported { kind: String },
}

impl Warning {
    /// What the warning tells the user: names only, never the values they failed to
    /// resolve (REQ-VAR-07), the TLS state verbatim (REQ-HTTP-10), and an auth that went
    /// out without credentials (REQ-HTTP-06). The one wording every surface shows — the
    /// Console's entries and the editor's own hint.
    pub fn user_message(&self) -> String {
        match self {
            Warning::UnresolvedVariables(names) => {
                format!("Unresolved variables: {}", names.join(", "))
            }
            Warning::TlsVerifyDisabled => {
                "TLS certificate verification is disabled for this request.".to_string()
            }
            Warning::AuthIncomplete { kind, missing } => format!(
                "The {kind} auth was not applied because its {missing} is empty. \
                 The request was sent without credentials."
            ),
            Warning::AuthUnsupported { kind } => format!(
                "The \"{kind}\" auth type is not supported yet. \
                 The request was sent without credentials."
            ),
        }
    }

    /// The warning for an auth that added no credential, or `None` when the auth did
    /// what it was configured to do (REQ-HTTP-06).
    pub(crate) fn for_auth(auth: &AppliedAuth) -> Option<Self> {
        match auth {
            AppliedAuth::None | AppliedAuth::Applied => None,
            AppliedAuth::Incomplete { kind, missing } => {
                Some(Warning::AuthIncomplete { kind, missing })
            }
            AppliedAuth::Unsupported { kind } => {
                Some(Warning::AuthUnsupported { kind: kind.clone() })
            }
        }
    }

    /// Add `names` to the [`Warning::UnresolvedVariables`] in `warnings`, after the names
    /// it already lists and without duplicates — creating it, first, when there is none —
    /// so one warning names every unresolved variable of the send.
    pub(crate) fn add_unresolved(warnings: &mut Vec<Warning>, names: Vec<String>) {
        if names.is_empty() {
            return;
        }
        let existing = warnings.iter_mut().find_map(|warning| match warning {
            Warning::UnresolvedVariables(listed) => Some(listed),
            _ => None,
        });
        match existing {
            Some(listed) => {
                for name in names {
                    if !listed.contains(&name) {
                        listed.push(name);
                    }
                }
            }
            None => warnings.insert(0, Warning::UnresolvedVariables(names)),
        }
    }
}

/// Why [`resolve_request`] produced no request.
///
/// `Display` and `Debug` are both log-safe: error logging formats both (`{error:#}` and
/// `.log_err()`), the derived [`Debug`] delegates to [`TransportError`]'s hand-written
/// redacting one, `Display` forwards the redacted text, and [`ResolveError`] names
/// variables, never values. The UI gets the full text through
/// [`TransportError::user_message`] — UI only, never logged.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PipelineError {
    /// Variables reference each other in a loop or nest too deep: nothing resolves
    /// (REQ-VAR-06). [`variables::resolve`] names the cycle.
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    /// Building the request to send failed: the resolved URL text did not parse
    /// (PRD §8.6), carried by [`TransportError::InvalidUrl`] as [`ResolvedRequest::new`]
    /// produced it.
    #[error(transparent)]
    Transport(#[from] TransportError),
}

/// Resolve every `{{variable}}` of `request` against `scopes`, generating dynamic values
/// through `dynamic`.
///
/// The resolved query params land on the URL's query after whatever it already carried.
/// Disabled rows are not sent. Resolving a name that nothing defines is a warning, not an
/// error: the request still goes out with the placeholder left literal (REQ-VAR-07).
///
/// # Errors
///
/// [`PipelineError::Resolve`] when the variables form a cycle or nest too deep
/// (REQ-VAR-06), and [`PipelineError::Transport`] when the resolved URL text does not
/// parse. No request is produced in either case.
pub fn resolve_request(
    request: &HttpRequest,
    scopes: &ScopeStack,
    dynamic: &mut DynamicValues,
) -> Result<(ResolvedRequest, Vec<Warning>), PipelineError> {
    let mut unresolved = Vec::new();
    let url_text = resolve_text(&request.url, scopes, dynamic, &mut unresolved)?;
    let params = resolve_pairs(&request.params, scopes, dynamic, &mut unresolved)?;
    let headers = resolve_pairs(&request.headers, scopes, dynamic, &mut unresolved)?
        .into_iter()
        .map(|(name, value)| Header::new(name, value))
        .collect();
    let body = resolve_body(&request.body, scopes, dynamic, &mut unresolved)?;

    let mut resolved = ResolvedRequest::new(
        request.method.clone(),
        &url_text,
        headers,
        body,
        send_settings(request.settings.as_ref()),
    )?;
    append_params(&mut resolved.url, params);

    let mut warnings = Vec::new();
    if !unresolved.is_empty() {
        warnings.push(Warning::UnresolvedVariables(unresolved));
    }
    if !resolved.settings.verify_tls {
        warnings.push(Warning::TlsVerifyDisabled);
    }
    Ok((resolved, warnings))
}

/// Resolve one template and record the names that stayed unresolved, so one warning can
/// name them across the whole request.
fn resolve_text(
    template: &str,
    scopes: &ScopeStack,
    dynamic: &mut DynamicValues,
    unresolved: &mut Vec<String>,
) -> Result<String, ResolveError> {
    let resolved = resolve(template, scopes, dynamic)?;
    for name in resolved.unresolved {
        if !unresolved.contains(&name) {
            unresolved.push(name);
        }
    }
    // `resolved.secrets_used` needs no handling here: substitution is the same either
    // way, and carrying the values for downstream redaction is S13c's `prepare`.
    Ok(resolved.text)
}

/// Resolve the enabled rows of a request table — params, headers, urlencoded fields —
/// into `key, value` pairs in table order. A row without a value resolves as empty.
fn resolve_pairs(
    entries: &[KeyValue],
    scopes: &ScopeStack,
    dynamic: &mut DynamicValues,
    unresolved: &mut Vec<String>,
) -> Result<Vec<(String, String)>, ResolveError> {
    let mut pairs = Vec::new();
    for entry in entries.iter().filter(|entry| entry.enabled) {
        let key = resolve_text(&entry.key, scopes, dynamic, unresolved)?;
        let value = resolve_text(
            entry.value.as_deref().unwrap_or(""),
            scopes,
            dynamic,
            unresolved,
        )?;
        pairs.push((key, value));
    }
    Ok(pairs)
}

/// Append the resolved params to the URL query. A table that adds nothing must leave the
/// query exactly as it resolved, so an untouched URL gains no bare `?`.
///
/// The editor mirrors every enabled query row into the URL text (`request_editor`'s
/// Params tab), so a row the URL already carries is not appended again; each URL pair
/// absorbs one matching row, which keeps repeated keys (`?tag=a&tag=b`) exact.
fn append_params(url: &mut Url, params: Vec<(String, String)>) {
    let mut carried: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let params: Vec<(String, String)> = params
        .into_iter()
        .filter(
            |param| match carried.iter().position(|pair| pair == param) {
                Some(index) => {
                    carried.swap_remove(index);
                    false
                }
                None => true,
            },
        )
        .collect();
    if params.is_empty() {
        return;
    }
    let mut query = url.query_pairs_mut();
    for (key, value) in params {
        query.append_pair(&key, &value);
    }
}

/// The body to send with its text resolved: raw and urlencoded text, and every enabled
/// `form-data` field's name, value, file reference and content type. A file reference
/// resolves as text here and becomes a concrete path later (`body::resolve_body_files`,
/// REQ-HTTP-05); the unmodelled modes send nothing.
fn resolve_body(
    body: &Body,
    scopes: &ScopeStack,
    dynamic: &mut DynamicValues,
    unresolved: &mut Vec<String>,
) -> Result<RequestBody, ResolveError> {
    let resolved = match body {
        Body::None => RequestBody::None,
        Body::Raw { text, .. } => {
            RequestBody::Bytes(resolve_text(text, scopes, dynamic, unresolved)?.into_bytes())
        }
        Body::UrlEncoded { fields, .. } => {
            let pairs = resolve_pairs(fields, scopes, dynamic, unresolved)?;
            // `form_urlencoded` percent-encodes each value's own `&` and `=`, so one
            // field can never split into two on the wire.
            let mut form = form_urlencoded::Serializer::new(String::new());
            form.extend_pairs(pairs);
            RequestBody::Bytes(form.finish().into_bytes())
        }
        Body::Binary { file, .. } => RequestBody::File(PathBuf::from(file)),
        Body::FormData { fields, .. } => {
            let mut multipart = Vec::new();
            // A disabled field is not sent, so it resolves to nothing (REQ-HTTP-04).
            for field in fields.iter().filter(|field| field.enabled) {
                let name = resolve_text(&field.key, scopes, dynamic, unresolved)?;
                let content_type = field
                    .content_type
                    .as_deref()
                    .map(|template| resolve_text(template, scopes, dynamic, unresolved))
                    .transpose()?;
                // A field with a file sends the file whatever its value says — the file
                // is the payload (REQ-HTTP-05).
                let value = match field.file.as_deref().filter(|file| !file.is_empty()) {
                    Some(file) => MultipartValue::File(PathBuf::from(resolve_text(
                        file, scopes, dynamic, unresolved,
                    )?)),
                    None => MultipartValue::Text(resolve_text(
                        field.value.as_deref().unwrap_or(""),
                        scopes,
                        dynamic,
                        unresolved,
                    )?),
                };
                multipart.push(MultipartField {
                    name,
                    content_type,
                    value,
                });
            }
            RequestBody::Multipart(multipart)
        }
        Body::Unsupported(_) => RequestBody::None,
    };
    Ok(resolved)
}

/// The request's own setting overrides on top of [`SendSettings::default`]; a `None`
/// means "inherit", so the field keeps its default. `RequestSettings::remove_referer`
/// has no [`SendSettings`] field and is not applied here.
fn send_settings(overrides: Option<&RequestSettings>) -> SendSettings {
    let mut settings = SendSettings::default();
    let Some(overrides) = overrides else {
        return settings;
    };
    if let Some(timeout_ms) = overrides.timeout_ms {
        settings.timeout = Duration::from_millis(timeout_ms);
    }
    if let Some(follow_redirects) = overrides.follow_redirects {
        settings.follow_redirects = follow_redirects;
    }
    if let Some(max_redirects) = overrides.max_redirects {
        settings.max_redirects = max_redirects;
    }
    if let Some(follow_original_method) = overrides.follow_original_method {
        settings.follow_original_method = follow_original_method;
    }
    if let Some(keep_authorization) = overrides.keep_authorization {
        settings.keep_authorization = keep_authorization;
    }
    if let Some(verify_tls) = overrides.verify_tls {
        settings.verify_tls = verify_tls;
    }
    if let Some(encode_url) = overrides.encode_url {
        settings.encode_url = encode_url;
    }
    if let Some(http_version) = overrides.http_version {
        settings.http_version = http_version;
    }
    if let Some(disable_cookie_jar) = overrides.disable_cookie_jar {
        settings.disable_cookie_jar = disable_cookie_jar;
    }
    settings
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "pipeline_param_mirror_tests.rs"]
mod param_mirror_tests;

#[cfg(test)]
#[path = "pipeline_req_http_09_tests.rs"]
mod req_http_09_tests;
