//! The Console feed (REQ-CON-01/03/04): a real send lands one entry whose summary and raw
//! request show the resolved request as it went on the wire — redacted — and a request
//! that turns TLS verification off lands a Warning (REQ-HTTP-10).

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{
    Body, HttpRequest, ItemId, KeyValue, Method, RequestSettings, Variable, VariableKind,
};
use gpui::{AppContext, TestAppContext};
use http_engine::{FakeTransport, HttpClient, SendInput};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use variables::{ScopeKind, ScopeStack};

use crate::{ConsoleLevel, ConsoleStore, record_outcome};

/// The one obviously-fake secret value these tests store and expect masked.
const SECRET: &str = "token-REDACTED-example";

/// The mask every redacted occurrence becomes (`api_model::redact_text`).
const MASK: &str = "••••••";

/// A real send records one entry whose summary names the method, URL and status — what
/// the Console's summary line shows (REQ-CON-01).
#[gpui::test]
async fn send_creates_entry_with_summary(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let input = send_input(
        plain_request(),
        env_scopes(Vec::new()),
        Arc::new(MemorySecretStore::new()),
    );

    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap();

    cx.update(|cx| {
        init_console(cx);
        record_outcome(&outcome, cx);

        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(entries.len(), 1, "one send lands exactly one entry");
        let entry = entries.front().expect("the one entry just pushed");
        assert_eq!(entry.method, Some(Method::Get));
        assert_eq!(entry.url, "https://example.test/users");
        assert_eq!(entry.status, Some(200));
    });
}

/// The raw view shows the resolved request exactly as it went on the wire — the variables
/// filled in, the header values real — and a secret variable's value appears only as the
/// redaction mask (REQ-CON-03, REQ-NFR-04).
#[gpui::test]
async fn raw_view_shows_resolved_request_exactly(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let request = HttpRequest {
        url: "https://{{host}}/users".to_string(),
        headers: vec![entry("X-Trace", "{{trace}}"), entry("X-Token", "{{token}}")],
        ..plain_request()
    };
    let secrets = MemorySecretStore::new();
    secrets
        .set(&key("token"), &SecretValue::new(SECRET))
        .unwrap();
    let mut scopes = env_scopes(vec![
        variable("host", "api.example.test"),
        variable("trace", "trace-4f8a"),
        secret("token"),
    ]);
    scopes.push(ScopeKind::Environment, Vec::new());
    let input = send_input(request, scopes, Arc::new(secrets));

    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap();

    cx.update(|cx| {
        init_console(cx);
        record_outcome(&outcome, cx);

        let entries = cx.global::<ConsoleStore>().entries();
        let entry = entries.front().expect("the one entry just pushed");
        assert_eq!(
            entry.raw_request,
            format!(
                "GET /users HTTP/1.1\r\n\
                 Host: api.example.test\r\n\
                 X-Trace: trace-4f8a\r\n\
                 X-Token: {MASK}\r\n\
                 User-Agent: Pustman/{}\r\n\
                 Accept: */*\r\n\
                 Accept-Encoding: gzip,deflate,br\r\n\r\n",
                env!("CARGO_PKG_VERSION")
            ),
            "the raw text is the resolved HTTP/1.1 request, with the secret masked"
        );
        assert!(
            !entry.raw_request.contains(SECRET),
            "the secret value appears only as the mask"
        );
    });
}

/// A request that turns TLS certificate verification off lands a Warning-level entry
/// (REQ-CON-04, REQ-HTTP-10).
#[gpui::test]
async fn tls_disabled_warning_logged(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let request = HttpRequest {
        settings: Some(RequestSettings {
            verify_tls: Some(false),
            ..RequestSettings::default()
        }),
        ..plain_request()
    };
    let input = send_input(
        request,
        env_scopes(Vec::new()),
        Arc::new(MemorySecretStore::new()),
    );

    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .unwrap();

    cx.update(|cx| {
        init_console(cx);
        record_outcome(&outcome, cx);

        let entries = cx.global::<ConsoleStore>().entries();
        assert!(
            entries.iter().any(|entry| {
                entry.level == ConsoleLevel::Warning && entry.raw_response.contains("TLS")
            }),
            "turning TLS verification off lands a Warning entry naming it"
        );
    });
}

/// Install the settings and the [`ConsoleStore`] global the feed pushes into.
fn init_console(cx: &mut gpui::App) {
    let init = settings::SettingsStore::init(cx, None);
    assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
    crate::init(cx);
}

/// A GET for `https://example.test/users` with no headers, no body and default settings.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: api_model::Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// One enabled header row.
fn entry(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// One enabled shared variable with a value.
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

/// An enabled secret as a workspace file records it: no value at all (REQ-VAR-SHARE-02).
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

/// Everything `HttpClient::send_request` needs for one send.
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

/// The keychain key every test's lookup hands the pipeline: ids only the caller knows.
fn key(name: &str) -> SecretKey {
    SecretKey::new("workspace-test", "environment-test", name).unwrap()
}

/// The tests' `SendInput::secret_key`: one keychain entry per variable name.
fn lookup(_kind: ScopeKind, name: &str) -> Option<SecretKey> {
    Some(key(name))
}
