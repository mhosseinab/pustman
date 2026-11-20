//! History recording (PRD §8.5, REQ-HIST-01..03): every send leaves one row, the row
//! holds the pre-resolution request, no secret value ever reaches the SQLite file, the
//! body prefix is capped at one megabyte, and a cancelled send is recorded as cancelled.
//! Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{
    Auth, Body, HttpRequest, ItemId, KeyValue, Method, RawLanguage, SecretValues, Variable,
    VariableKind, redact_text,
};
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{AppContext, TestAppContext};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{HistoryEntry, LocalStore};
use variables::{ScopeKind, ScopeStack};

use crate::{
    FakeTransport, HISTORY_BODY_LIMIT, Header, HistoryRecorder, HttpClient, PipelineError,
    ResolvedRequest, Response, ResponseBody, SendInput, Transport, TransportError,
};

/// The one obviously-fake secret value the canary test stores and scans the database for.
const CANARY: &str = "token-REDACTED-example-canary-0042";

/// An obviously-fake literal token, typed into a request instead of referenced.
const LITERAL_TOKEN: &str = "token-REDACTED-literal-1337";

/// The workspace every test records into.
const WORKSPACE: &str = "workspace-under-test";

/// The row restores the request as it was before variable resolution (REQ-HIST-02):
/// `{{host}}` is stored literally, not the value it resolved to.
#[gpui::test]
async fn history_stores_pre_resolution_request(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let request = HttpRequest {
        url: "https://{{host}}/users".to_string(),
        ..plain_request()
    };
    let input = send_input(
        request,
        env_scopes(vec![variable("host", "example.test")]),
        Arc::new(MemorySecretStore::new()),
    );

    client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect("the send succeeds");

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is recorded");
    assert_eq!(
        rows[0].request_json.get("url").and_then(|url| url.as_str()),
        Some("https://{{host}}/users"),
        "the stored request keeps `{{host}}` literally"
    );
    assert!(
        rows[0].target.contains("{{host}}"),
        "the target is the pre-resolution URL, redacted"
    );
}

/// No secret value reaches the SQLite file (REQ-HIST-03): the request references the
/// secret in a header, the URL query, the body and the bearer auth, and the response
/// echoes the value in its body and a `Set-Cookie`. Every byte of every file the
/// database writes is scanned for the canary.
#[gpui::test]
async fn history_never_contains_secret_value(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp directory opens");
    let mut opened =
        LocalStore::open(directory.path().join("local-store.sqlite")).expect("the store opens");
    opened.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(opened));
    let response = Response {
        headers: vec![Header::new("Set-Cookie", format!("sid={CANARY}; Path=/"))],
        ..Response::new(
            200,
            "OK",
            ResponseBody::Bytes(format!("echo {CANARY}").into_bytes()),
        )
    };
    let client = cx.new(|_| HttpClient::new(Arc::new(CannedTransport { response })));
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .set(&key("token"), &SecretValue::new(CANARY))
        .expect("the secret stores");
    let mut request = plain_request();
    request.url = "https://example.test/echo?q={{token}}".to_string();
    request.headers = vec![entry("X-Trace", "{{token}}")];
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: "{{token}}".to_string(),
        extra: BTreeMap::new(),
    };
    request.auth = Auth::Bearer {
        token: Some("{{token}}".to_string()),
        extra: BTreeMap::new(),
    };
    let input = send_input(request, env_scopes(vec![secret("token")]), secrets);

    client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect("the send succeeds");

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is recorded");
    let prefix = rows[0]
        .response_body_prefix
        .as_ref()
        .expect("a body prefix is stored");
    assert!(
        contains(prefix, b"echo"),
        "the stored prefix holds the echoed body"
    );
    for file in std::fs::read_dir(directory.path()).expect("the temp directory lists") {
        let bytes = std::fs::read(file.expect("the directory entry").path())
            .expect("the database file reads");
        assert!(
            !contains(&bytes, CANARY.as_bytes()),
            "the secret value reached a database file"
        );
    }
}

/// A response body over the cap is stored at most one megabyte (REQ-HIST-02), and the
/// summary still reports the whole body's size.
#[gpui::test]
async fn history_body_capped_at_one_megabyte(cx: &mut TestAppContext) {
    let body = vec![b'x'; 3 * 1024 * 1024];
    let transport = FakeTransport::with_response(cx.executor(), 200, body);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let input = send_input(
        plain_request(),
        env_scopes(Vec::new()),
        Arc::new(MemorySecretStore::new()),
    );

    client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect("the send succeeds");

    let rows = stored_rows(&store);
    let prefix = rows[0]
        .response_body_prefix
        .as_ref()
        .expect("a body prefix is stored");
    assert!(prefix.len() <= HISTORY_BODY_LIMIT, "the body is capped");
    assert_eq!(prefix.len(), HISTORY_BODY_LIMIT, "the cap is one megabyte");
    let summary = rows[0]
        .response_summary_json
        .as_ref()
        .expect("a response summary is stored");
    assert_eq!(
        summary.get("body_size"),
        Some(&serde_json::json!(3 * 1024 * 1024)),
        "the summary reports the whole body"
    );
}

/// A send cancelled before it finished is recorded as cancelled (REQ-HIST-01 records
/// every send): one row, no status, and the cancelled marker in the summary.
#[gpui::test]
async fn cancelled_send_recorded_as_cancelled(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let input = send_input(
        plain_request(),
        env_scopes(Vec::new()),
        Arc::new(MemorySecretStore::new()),
    );

    let waiter = client.update(cx, |client, cx| client.send_request(input, cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.executor().run_until_parked();
    let outcome = waiter.await;
    assert!(
        matches!(
            outcome,
            Err(PipelineError::Transport(TransportError::Cancelled))
        ),
        "the cancelled send resolves as cancelled"
    );

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the cancelled send is recorded once");
    assert_eq!(rows[0].status, None);
    assert_eq!(
        rows[0].response_summary_json,
        Some(serde_json::json!({ "cancelled": true })),
    );
}

/// A literal bearer token is masked while a `{{token}}` reference stays as written: the
/// reference is restorable (REQ-HIST-02) and its resolved value is masked through the
/// send's secrets anyway (REQ-HIST-03), while a literal credential only survives as the
/// shared mask.
#[gpui::test]
async fn history_masks_literal_bearer_token_but_keeps_a_reference(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    transport.push_response(Duration::ZERO, 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .set(&key("token"), &SecretValue::new(CANARY))
        .expect("the secret stores");

    let mut reference_request = plain_request();
    reference_request.auth = Auth::Bearer {
        token: Some("{{token}}".to_string()),
        extra: BTreeMap::new(),
    };
    client
        .update(cx, |client, cx| {
            client.send_request(
                send_input(
                    reference_request,
                    env_scopes(vec![secret("token")]),
                    secrets.clone(),
                ),
                cx,
            )
        })
        .await
        .expect("the send succeeds");

    let mut literal_request = plain_request();
    literal_request.auth = Auth::Bearer {
        token: Some(LITERAL_TOKEN.to_string()),
        extra: BTreeMap::new(),
    };
    client
        .update(cx, |client, cx| {
            client.send_request(
                send_input(literal_request, env_scopes(Vec::new()), secrets.clone()),
                cx,
            )
        })
        .await
        .expect("the send succeeds");

    let stored_tokens: Vec<String> = stored_rows(&store)
        .iter()
        .map(|row| {
            row.request_json["auth"]["token"]
                .as_str()
                .expect("the stored token is text")
                .to_string()
        })
        .collect();
    let shared_mask = redact_text(LITERAL_TOKEN, &SecretValues::from_values([LITERAL_TOKEN]));
    assert!(
        stored_tokens.contains(&"{{token}}".to_string()),
        "a reference stays as written"
    );
    assert!(
        stored_tokens.contains(&shared_mask),
        "a literal credential becomes the shared mask"
    );
    assert!(!stored_tokens.contains(&LITERAL_TOKEN.to_string()));
}

/// A failed send is recorded with the error's variant as a fixed kind string — never the
/// error text, which can quote the resolved URL or a path (PRD §7).
#[gpui::test]
async fn history_records_failed_send_with_error_kind(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: "example.test".to_string(),
        },
    );
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let input = send_input(
        plain_request(),
        env_scopes(Vec::new()),
        Arc::new(MemorySecretStore::new()),
    );

    let error = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect_err("the send fails");
    assert!(matches!(
        error,
        PipelineError::Transport(TransportError::Dns { .. })
    ));

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the failed send is recorded");
    assert_eq!(rows[0].status, None);
    assert_eq!(
        rows[0].response_summary_json,
        Some(serde_json::json!({ "error": "dns" })),
    );
}

/// A transport that answers one canned response: [`FakeTransport`] scripts no response
/// headers, and the canary test needs a `Set-Cookie` (test helpers are never edited by a
/// later step, so the double lives here).
struct CannedTransport {
    response: Response,
}

impl Transport for CannedTransport {
    fn send(
        &self,
        _request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let response = self.response.clone();
        async move { Ok(response) }.boxed()
    }
}

/// One GET to `https://example.test/users` with no params, no body and no own auth.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "history test".to_string(),
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

/// The keychain key every test's lookup hands `prepare`: ids only the caller knows.
fn key(name: &str) -> SecretKey {
    SecretKey::new("workspace-test", "environment-test", name).expect("the key is valid")
}

/// The tests' [`SendInput::secret_key`]: one keychain entry per variable name.
fn lookup(_kind: ScopeKind, name: &str) -> Option<SecretKey> {
    Some(key(name))
}

/// The in-memory local store the recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// What the `history` table holds, read back through `storage`.
fn stored_rows(store: &Mutex<LocalStore>) -> Vec<HistoryEntry> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list")
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
