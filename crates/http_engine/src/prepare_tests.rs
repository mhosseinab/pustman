//! [`prepare`] before a send: every secret the request references is looked up first, one
//! with no value fails as [`TransportError::UnresolvedSecret`] before the transport sees a
//! call, and the values the send substitutes reach the wire and the redaction set — never
//! a log line (PRD §7, REQ-VAR-SHARE-02).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use api_model::{
    Auth, Body, HttpRequest, ItemId, KeyValue, Method, RawLanguage, Variable, VariableKind,
    redact_text,
};
use gpui::{AppContext, TestAppContext};
use secrets::{MemorySecretStore, SecretError, SecretKey, SecretStore, SecretValue};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::{
    AppliedAuth, FakeTransport, Header, HttpClient, PipelineError, RequestBody, SendInput,
    TransportError, prepare,
};

/// The one obviously-fake secret value these tests store and expect.
const SECRET: &str = "token-REDACTED-example";

/// The mask every redacted occurrence becomes (`api_model::redact_text`).
const MASK: &str = "••••••";

/// An unset secret aborts the send before the network: the transport sees zero calls, so
/// the placeholder goes out neither as an empty string nor literally (REQ-VAR-SHARE-02).
#[gpui::test]
async fn test_missing_secret_fails_before_network(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let input = send_input(
        request_with_header("Authorization", "Bearer {{token}}"),
        env_scopes(vec![secret("token")]),
        Arc::new(CountingStore::new()),
    );

    let error = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        PipelineError::Transport(TransportError::UnresolvedSecret {
            names: vec!["token".to_string()]
        })
    );
    assert!(
        transport.requests().is_empty(),
        "the transport saw zero calls"
    );
}

/// The store's value reaches the wire exactly as stored, and [`crate::Prepared`] hands the
/// value on for redaction: [`redact_text`] masks it wherever the send is shown or stored.
#[gpui::test]
async fn test_secret_from_store_reaches_the_wire_and_secrets_used(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = CountingStore::new();
    store.insert("token", SECRET);
    let input = send_input(
        request_with_header("Authorization", "Bearer {{token}}"),
        env_scopes(vec![secret("token")]),
        Arc::new(store),
    );

    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap();

    let sent = transport.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].headers,
        vec![Header::new("Authorization", format!("Bearer {SECRET}"))]
    );
    assert_eq!(
        redact_text(&format!("Bearer {SECRET}"), &outcome.secrets_used),
        format!("Bearer {MASK}")
    );
}

/// A secret referenced only through another variable's value is looked up just the same:
/// `{{auth_header}}` → `Bearer {{token}}` substitutes both at send time (REQ-VAR-01).
#[test]
fn test_secret_referenced_through_another_variable_is_looked_up() {
    let store = CountingStore::new();
    store.insert("token", SECRET);
    let scopes = env_scopes(vec![
        variable("auth_header", "Bearer {{token}}"),
        secret("token"),
    ]);
    let request = request_with_header("Authorization", "{{auth_header}}");
    let mut dynamic = DynamicValues::new();

    let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

    assert_eq!(
        prepared.resolved.headers,
        vec![Header::new("Authorization", format!("Bearer {SECRET}"))]
    );
    assert_eq!(
        redact_text(&format!("Bearer {SECRET}"), &prepared.secrets_used),
        format!("Bearer {MASK}")
    );
}

/// A secret referenced from any request field is looked up and substituted where it is
/// used: URL, params, headers and body text all resolve alike (REQ-VAR-01).
#[test]
fn test_secret_referenced_in_any_request_field_is_looked_up() {
    for placement in ["url", "param", "header", "body"] {
        let store = CountingStore::new();
        store.insert("token", SECRET);
        let scopes = env_scopes(vec![secret("token")]);
        let mut request = plain_request();
        match placement {
            "url" => request.url = "https://example.test/{{token}}".to_string(),
            "param" => request.params = vec![entry("q", "{{token}}")],
            "header" => request.headers = vec![entry("X-Api-Key", "{{token}}")],
            "body" => {
                request.body = Body::Raw {
                    language: RawLanguage::Json,
                    text: "{{token}}".to_string(),
                    extra: BTreeMap::new(),
                }
            }
            _ => unreachable!("every placement is named above"),
        }
        let mut dynamic = DynamicValues::new();

        let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

        match placement {
            "url" => assert_eq!(prepared.resolved.url.path(), format!("/{SECRET}")),
            "param" => assert_eq!(
                prepared.resolved.url.query(),
                Some(format!("q={SECRET}").as_str())
            ),
            "header" => assert_eq!(
                prepared.resolved.headers,
                vec![Header::new("X-Api-Key", SECRET)]
            ),
            "body" => assert_eq!(
                prepared.resolved.body,
                RequestBody::Bytes(SECRET.as_bytes().to_vec())
            ),
            _ => unreachable!("every placement is named above"),
        }
        assert_eq!(
            store.get_count("token"),
            1,
            "{placement}: one lookup per send"
        );
        assert_eq!(
            redact_text(SECRET, &prepared.secrets_used),
            MASK,
            "{placement}: the value is in the redaction set"
        );
    }
}

/// A secret only the effective auth references is looked up: `Bearer {{token}}` in the
/// auth tab resolves like any other field, own or inherited (REQ-HTTP-06).
#[test]
fn test_secret_only_in_auth_field_is_looked_up() {
    let store = CountingStore::new();
    store.insert("token", SECRET);
    let mut request = plain_request();
    request.auth = Auth::Bearer {
        token: Some("{{token}}".to_string()),
        extra: BTreeMap::new(),
    };
    let scopes = env_scopes(vec![secret("token")]);
    let mut dynamic = DynamicValues::new();

    let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

    assert_eq!(
        prepared.resolved.headers,
        vec![Header::new("Authorization", format!("Bearer {SECRET}"))]
    );
    assert_eq!(prepared.auth, AppliedAuth::Applied);
    assert_eq!(store.get_count("token"), 1);
}

/// A secret the request never references is never read: every keychain read can prompt the
/// user (macOS), so one send touches only the names it substitutes.
#[test]
fn test_unreferenced_secret_is_never_read() {
    let store = CountingStore::new();
    store.insert("token", SECRET);
    store.insert("other", "other-secret-REDACTED-example");
    let request = request_with_header("Authorization", "Bearer {{token}}");
    let scopes = env_scopes(vec![secret("token"), secret("other")]);
    let mut dynamic = DynamicValues::new();

    let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

    assert_eq!(store.get_count("token"), 1);
    assert_eq!(
        store.get_count("other"),
        0,
        "an unreferenced secret is never read"
    );
    assert_eq!(
        prepared.resolved.headers[0].value,
        format!("Bearer {SECRET}")
    );
}

/// An auth kind outside the M1 subset is reported in the outcome and the send still runs:
/// the user is told "auth type not supported yet" instead of sending silently (REQ-HTTP-06).
#[gpui::test]
async fn test_unsupported_auth_is_reported_in_outcome(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let mut request = plain_request();
    request.auth = Auth::Unsupported(serde_json::json!({ "type": "hawk", "id": "demo" }));
    let input = send_input(request, ScopeStack::new(), Arc::new(CountingStore::new()));

    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap();

    assert_eq!(
        outcome.auth,
        AppliedAuth::Unsupported {
            kind: "hawk".to_string()
        }
    );
    assert_eq!(outcome.response.status, 200);
    assert_eq!(
        transport.requests().len(),
        1,
        "the send still runs; the outcome carries the report"
    );
}

/// A secret the caller already holds a local value for is never read from the store: the
/// local value wins (REQ-VAR-SHARE-01) and is what reaches the wire and the redaction set.
#[test]
fn test_secret_with_a_local_value_is_not_read_from_the_store() {
    const LOCAL: &str = "local-value-REDACTED-example";
    let store = CountingStore::new();
    store.insert("token", "store-value-REDACTED-example");
    let mut scopes =
        ScopeStack::with_local_values(BTreeMap::from([("token".to_string(), LOCAL.to_string())]));
    scopes.push(ScopeKind::Environment, vec![secret("token")]);
    let request = request_with_header("Authorization", "Bearer {{token}}");
    let mut dynamic = DynamicValues::new();

    let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

    assert_eq!(
        store.get_count("token"),
        0,
        "a local value is never looked up"
    );
    assert_eq!(
        prepared.resolved.headers,
        vec![Header::new("Authorization", format!("Bearer {LOCAL}"))]
    );
    assert_eq!(
        redact_text(&format!("Bearer {LOCAL}"), &prepared.secrets_used),
        format!("Bearer {MASK}")
    );
}

/// [`crate::Prepared`]'s `Debug` carries no value: `SecretValues` prints counts and
/// `ResolvedRequest` redacts, so what a failure prints can be logged as it is.
#[test]
fn test_prepared_debug_never_prints_secret_values() {
    let store = CountingStore::new();
    store.insert("token", SECRET);
    let request = request_with_header("Authorization", "Bearer {{token}}");
    let scopes = env_scopes(vec![secret("token")]);
    let mut dynamic = DynamicValues::new();

    let prepared = prepare(&request, &[], &scopes, &store, &lookup, &mut dynamic).unwrap();

    let debug = format!("{prepared:?}");
    assert!(
        !debug.contains(SECRET),
        "the debug output carried the secret value: {debug}"
    );
}

/// One GET to `https://example.test/users` with no params, no body and no own auth.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "prepare test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// The same request with one enabled header row.
fn request_with_header(name: &str, value: &str) -> HttpRequest {
    HttpRequest {
        headers: vec![entry(name, value)],
        ..plain_request()
    }
}

/// One enabled param or header row.
fn entry(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// One enabled shared variable with a shared value.
fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// An enabled secret as a workspace file records it: no value at all
/// (REQ-VAR-SHARE-02).
fn secret(key: &str) -> Variable {
    Variable {
        kind: VariableKind::Secret,
        value: None,
        ..variable(key, "")
    }
}

/// A scope stack holding `variables` in the environment scope.
fn env_scopes(variables: Vec<Variable>) -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Environment, variables);
    scopes
}

/// Everything [`HttpClient::send_request`] needs for one send.
fn send_input(
    request: HttpRequest,
    scopes: ScopeStack,
    secrets: Arc<dyn SecretStore>,
) -> SendInput {
    SendInput {
        request,
        ancestors: Vec::new(),
        scopes,
        secrets,
        secret_key: Arc::new(lookup),
    }
}

/// The keychain key every test's lookup hands [`prepare`]: ids only the caller knows.
fn key(name: &str) -> SecretKey {
    SecretKey::new("workspace-test", "environment-test", name).unwrap()
}

/// The tests' [`SendInput::secret_key`]: one keychain entry per variable name.
fn lookup(_kind: ScopeKind, name: &str) -> Option<SecretKey> {
    Some(key(name))
}

/// A [`SecretStore`] that counts the reads of each variable name: proving that a secret
/// the request does not reference is never touched.
struct CountingStore {
    inner: MemorySecretStore,
    gets: Mutex<BTreeMap<String, usize>>,
}

impl CountingStore {
    fn new() -> Self {
        Self {
            inner: MemorySecretStore::new(),
            gets: Mutex::new(BTreeMap::new()),
        }
    }

    /// Store `value` under `name`'s keychain key.
    fn insert(&self, name: &str, value: &str) {
        self.inner
            .set(&key(name), &SecretValue::new(value))
            .unwrap();
    }

    /// How many times `name` was read.
    fn get_count(&self, name: &str) -> usize {
        lock(&self.gets).get(name).copied().unwrap_or(0)
    }
}

impl SecretStore for CountingStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        let mut gets = lock(&self.gets);
        *gets.entry(key.variable_key().to_string()).or_insert(0) += 1;
        drop(gets);
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)
    }
}

/// The counts lock. A poisoned lock still holds usable counts for a test.
fn lock(counts: &Mutex<BTreeMap<String, usize>>) -> MutexGuard<'_, BTreeMap<String, usize>> {
    counts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
