//! Preparing one send before the transport sees it: secrets first, then `{{variables}}`
//! and auth (PRD §7, REQ-VAR-SHARE-02, REQ-HTTP-06).
//!
//! [`prepare`] runs in the order that keeps an unset secret from ever reaching the wire.
//! It collects the secret names the request references — URL, enabled params and headers,
//! body text, the effective auth's fields, and the values those names resolve through —
//! reads the ones with no local value from the [`SecretStore`], and fails the whole send
//! as [`TransportError::UnresolvedSecret`] if any has none. Only then are variables
//! resolved and auth applied, so no partially prepared request exists (REQ-VAR-SHARE-02:
//! an unset secret is never sent as an empty string or a literal `{{name}}`).
//!
//! A secret value reaches this module for exactly two reasons: to be substituted into the
//! request, and to be carried in [`Prepared::secrets_used`] so everything downstream can
//! redact it. Nothing here logs, prints or formats one — not even a [`secrets::SecretError`],
//! whose text carries keychain ids.

use std::collections::{BTreeMap, BTreeSet};

use api_model::{
    Auth, Body, EffectiveAuth, HttpRequest, KeyValue, SecretValues, VariableKind, effective_auth,
};
use secrets::{SecretKey, SecretStore};
use variables::{Binding, DynamicValues, ScopeKind, ScopeStack, resolve, spans};

use crate::body;
use crate::error::TransportError;
use crate::pipeline::{PipelineError, Warning, resolve_request};
use crate::request::ResolvedRequest;
use crate::request_auth::{AppliedAuth, apply_auth, basic_credentials};

/// One request, ready for the transport.
///
/// `Debug` prints no values: [`ResolvedRequest`]'s `Debug` redacts and `secrets_used`
/// prints counts only, so a failure can log this as it is.
#[derive(Debug)]
pub struct Prepared {
    /// The request to send: every field resolved, auth applied.
    pub resolved: ResolvedRequest,
    /// What to tell the user: variables nothing resolved, TLS verification off
    /// (REQ-VAR-07, REQ-HTTP-10).
    pub warnings: Vec<Warning>,
    /// What the effective auth did. `Unsupported` and `Incomplete` added no credential;
    /// the caller reports them so no auth failure is ever sent silently (REQ-HTTP-06).
    pub auth: AppliedAuth,
    /// The values of every secret this send substitutes, for redaction downstream
    /// (history, Console, exports — `api_model::redact_text`). Never log them. A value
    /// too short or too mask-like to mask is refused by [`SecretValues`]; the caller that
    /// reports the send to the user warns when [`SecretValues::refused_count`] is not
    /// zero, because those values then appear unmasked.
    pub secrets_used: SecretValues,
}

/// Prepare one send: fetch the secrets it references, resolve its `{{variables}}`, apply
/// its auth.
///
/// `ancestors` are the request's parents' auths, nearest first; `secret_key` names the
/// keychain entry of a secret variable given the winning variable's scope kind and name —
/// only the caller knows the workspace and scope ids. `dynamic` is the send's one
/// [`DynamicValues`] (REQ-VAR-05); this call generates nothing it does not substitute.
///
/// # Errors
///
/// [`TransportError::UnresolvedSecret`] — as [`PipelineError::Transport`] — when a
/// referenced secret has no local value and no stored one: the names alone are reported,
/// in first-reference order and without duplicates, and the transport is never called.
/// [`PipelineError::Resolve`] when variables form a cycle or nest too deep (REQ-VAR-06),
/// [`PipelineError::Transport`] when the resolved URL text does not parse. Nothing is
/// sent in any of those cases.
pub fn prepare(
    request: &HttpRequest,
    ancestors: &[&Auth],
    scopes: &ScopeStack,
    secrets: &dyn SecretStore,
    secret_key: &dyn Fn(ScopeKind, &str) -> Option<SecretKey>,
    dynamic: &mut DynamicValues,
) -> Result<Prepared, PipelineError> {
    let chain: Vec<&Auth> = std::iter::once(&request.auth)
        .chain(ancestors.iter().copied())
        .collect();
    let (effective, _source) = effective_auth(&chain);
    let auth_templates = auth_field_templates(&effective);

    let mut references = ReferencedSecrets::new(scopes, secrets, secret_key);
    references.scan(&request.url);
    for template in row_templates(&request.params) {
        references.scan(template);
    }
    for template in row_templates(&request.headers) {
        references.scan(template);
    }
    match &request.body {
        Body::Raw { text, .. } => references.scan(text),
        Body::UrlEncoded { fields, .. } => {
            for template in row_templates(fields) {
                references.scan(template);
            }
        }
        // Every text a form-data field substitutes is scanned like any other, or a
        // secret in one would reach the wire as its literal placeholder (REQ-VAR-SHARE-02).
        Body::FormData { fields, .. } => {
            for field in fields.iter().filter(|field| field.enabled) {
                for template in [
                    field.key.as_str(),
                    field.value.as_deref().unwrap_or(""),
                    field.file.as_deref().unwrap_or(""),
                    field.content_type.as_deref().unwrap_or(""),
                ] {
                    references.scan(template);
                }
            }
        }
        // A binary body sends its file path unresolved: it carries no text
        // `resolve_request` substitutes.
        Body::None | Body::Binary { .. } | Body::Unsupported(_) => {}
    }
    for template in &auth_templates {
        references.scan(template);
    }
    if !references.missing.is_empty() {
        return Err(PipelineError::Transport(TransportError::UnresolvedSecret {
            names: references.missing,
        }));
    }
    let secrets_fetched = references.values;

    // Resolution reads one stack built from the caller's plus the fetched values: the
    // caller's `ScopeStack` has no insert API, and a rebuilt stack is the only way to put
    // a fetched secret value where `find` will resolve it without touching `variables`.
    let stack = scopes_with_secrets(scopes, &secrets_fetched);
    let (mut resolved, warnings) = resolve_request(request, &stack, dynamic)?;
    // The auth fields resolve against the same stack and the same `DynamicValues`, so one
    // `{{$guid}}` keeps one value across the request and its auth (REQ-VAR-05). They are
    // resolved here rather than in the closure below because that closure cannot carry a
    // `ResolveError`: a cycle in an auth field fails the send like one in any other field.
    let mut auth_texts = BTreeMap::new();
    for template in &auth_templates {
        auth_texts.insert(template.clone(), resolve(template, &stack, dynamic)?.text);
    }
    let auth = apply_auth(&mut resolved, &request.auth, ancestors, |template| {
        auth_texts
            .get(template)
            .cloned()
            // Unreachable: the templates resolved here are exactly the ones `apply_auth`
            // resolves. An empty field makes it report `Incomplete` instead of putting
            // placeholder text in a credential.
            .unwrap_or_default()
    });
    // The body mode's own headers are listed last: auth has added every header the user
    // did not type, and a user header of the same name wins over an auto one.
    resolved.auto_headers = body::auto_headers(
        &request.body,
        &resolved.body,
        &resolved.url,
        &resolved.headers,
    );
    let mut secret_values: Vec<String> = secrets_fetched
        .into_iter()
        .map(|(_name, value)| value)
        .collect();
    if auth == AppliedAuth::Applied
        && let EffectiveAuth::Basic { username, password } = &effective
    {
        // A server that echoes request headers (httpbin `/headers`) puts the Basic
        // credential's base64 form in its body; the resolved username and password
        // never match that derived string, so the exact string the auth sends is
        // registered too and its echo is masked everywhere the send is redacted.
        let resolved = |field: &Option<String>| {
            field
                .as_deref()
                .and_then(|template| auth_texts.get(template))
                .cloned()
                .unwrap_or_default()
        };
        secret_values.push(basic_credentials(&resolved(username), &resolved(password)));
    }
    let secrets_used = SecretValues::from_values(secret_values);
    Ok(Prepared {
        resolved,
        warnings,
        auth,
        secrets_used,
    })
}

/// The effective auth's field templates: the text [`apply_auth`] will resolve (PRD
/// REQ-MODEL-01). An unsupported kind contributes nothing — its raw object is never read,
/// because it can hold credentials.
fn auth_field_templates(effective: &EffectiveAuth) -> Vec<String> {
    let fields: Vec<Option<&String>> = match effective {
        EffectiveAuth::None | EffectiveAuth::Unsupported(_) => Vec::new(),
        EffectiveAuth::Basic { username, password } => vec![username.as_ref(), password.as_ref()],
        EffectiveAuth::Bearer { token } => vec![token.as_ref()],
        EffectiveAuth::ApiKey { key, value, .. } => vec![key.as_ref(), value.as_ref()],
    };
    fields.into_iter().flatten().cloned().collect()
}

/// The templates of a request table — params, headers, urlencoded fields: each enabled
/// row's key and value, in table order. A disabled row is not sent, so it references
/// nothing.
fn row_templates(rows: &[KeyValue]) -> Vec<&str> {
    rows.iter()
        .filter(|row| row.enabled)
        .flat_map(|row| [row.key.as_str(), row.value.as_deref().unwrap_or("")])
        .collect()
}

/// Every placeholder name of `template`, left to right. [`spans`] is the one placeholder
/// scanner in the workspace; this step only needs the names it lists.
fn names_in(template: &str, scopes: &ScopeStack) -> Vec<String> {
    spans(template, scopes)
        .into_iter()
        .map(|span| span.name)
        .collect()
}

/// The scope kind the winning variable of `name` lives in — [`SecretKey`] names a keychain
/// entry by scope — with the binding itself. The precedence rule is the one
/// [`ScopeStack::find`] applies: the narrowest enabled variable with this name wins.
fn bound_in<'a>(scopes: &'a ScopeStack, name: &str) -> Option<(ScopeKind, Binding<'a>)> {
    let binding = scopes.find(name)?;
    let scope = scopes.scopes().iter().find(|scope| {
        scope
            .variables
            .iter()
            .any(|variable| variable.enabled && variable.key == name)
    })?;
    Some((scope.kind, binding))
}

/// A [`ScopeStack`] that resolves against `scopes`' variables and reads values from the
/// caller's local values plus `secrets_fetched` (REQ-VAR-SHARE-01: a local value wins).
fn scopes_with_secrets(scopes: &ScopeStack, secrets_fetched: &[(String, String)]) -> ScopeStack {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    for scope in scopes.scopes() {
        for variable in &scope.variables {
            if let Some(local) = scopes.local_value(&variable.key) {
                values.insert(variable.key.clone(), local.to_string());
            }
        }
    }
    for (name, value) in secrets_fetched {
        values.insert(name.clone(), value.clone());
    }
    let mut stack = ScopeStack::with_local_values(values);
    for scope in scopes.scopes() {
        stack.push(scope.kind, scope.variables.clone());
    }
    stack
}

/// The secrets a request references, gathered in one pass before anything else runs.
///
/// Every name is followed once, through the values it resolves to: `{{a}}` whose value is
/// `Bearer {{token}}` references `token` (REQ-VAR-01 substitutes both). A secret is read
/// from the store only when its winning binding carries no local value — every keychain
/// read can prompt the user (macOS) — and only names the request actually references are
/// read at all.
struct ReferencedSecrets<'a> {
    scopes: &'a ScopeStack,
    secrets: &'a dyn SecretStore,
    secret_key: &'a dyn Fn(ScopeKind, &str) -> Option<SecretKey>,
    /// Every name already followed, so a shared or looping reference is handled once.
    visited: BTreeSet<String>,
    /// Referenced secrets that have a value, in first-reference order: the name and the
    /// value the send will substitute.
    values: Vec<(String, String)>,
    /// Referenced secrets with no value at all, in first-reference order.
    missing: Vec<String>,
}

impl<'a> ReferencedSecrets<'a> {
    fn new(
        scopes: &'a ScopeStack,
        secrets: &'a dyn SecretStore,
        secret_key: &'a dyn Fn(ScopeKind, &str) -> Option<SecretKey>,
    ) -> Self {
        Self {
            scopes,
            secrets,
            secret_key,
            visited: BTreeSet::new(),
            values: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Follow every reference of `template`, depth first and left to right, so the names
    /// of [`Self::missing`] come out in first-reference order. References inside a value
    /// are followed too — they resolve at send time just like the template's own.
    fn scan(&mut self, template: &str) {
        let scopes = self.scopes;
        let mut frames = vec![names_in(template, scopes).into_iter()];
        while !frames.is_empty() {
            // The borrow `frames.last_mut()` hands out ends with this statement; the
            // `follow` below pushes new frames.
            let next = frames.last_mut().and_then(|frame| frame.next());
            match next {
                Some(name) => self.follow(&name, &mut frames),
                None => {
                    frames.pop();
                }
            }
        }
    }

    /// Record `name` when it is a secret, and continue into the value it resolves to.
    fn follow(&mut self, name: &str, frames: &mut Vec<std::vec::IntoIter<String>>) {
        if DynamicValues::is_dynamic_name(name) || !self.visited.insert(name.to_string()) {
            return;
        }
        let scopes = self.scopes;
        let Some((kind, binding)) = bound_in(scopes, name) else {
            return;
        };
        let is_secret = binding.variable.kind == VariableKind::Secret;
        // A secret with a local value is never read: the caller's value wins
        // (REQ-VAR-SHARE-01) and every keychain read can prompt the user.
        let value = match binding.value {
            Some(local) => Some(local.to_string()),
            None if is_secret => self.read_secret(kind, name),
            None => None,
        };
        if let Some(value) = &value {
            if is_secret {
                self.values.push((name.to_string(), value.clone()));
            }
            frames.push(names_in(value, scopes).into_iter());
        }
    }

    /// The stored value of a referenced secret, or `None` when there is no value to send:
    /// no key for this scope, nothing stored, or a keychain that cannot be reached — every
    /// failure is one outcome, "unresolved", and only the name is ever reported. A
    /// [`secrets::SecretError`] is never stringified: its text carries keychain ids.
    fn read_secret(&mut self, kind: ScopeKind, name: &str) -> Option<String> {
        let secret_key = self.secret_key;
        let secrets = self.secrets;
        let value = secret_key(kind, name).and_then(|key| match secrets.get(&key) {
            Ok(value) => Some(value.expose().to_string()),
            Err(_error) => None,
        });
        if value.is_none() {
            self.missing.push(name.to_string());
        }
        value
    }
}

#[cfg(test)]
#[path = "prepare_tests.rs"]
mod tests;
