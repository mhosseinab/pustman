//! Redaction of a secret in its ENCODED forms and of credentials typed straight into the
//! Auth tab (S38 review). `redaction.rs` checks the Console only through `export_text`,
//! which redacts again, and its secret has no character an encoder rewrites; here the
//! STORED `ConsoleEntry` fields and the stored history row are checked, for secrets that
//! hold `+ / = @ " ` and a space.
//!
//! The transport is an in-test echo server (httpbin `/anything` shape): it answers with
//! the request's headers and form body serialized as JSON, and a `Location` that carries
//! the secret percent-encoded — the encoders a real server applies, run for real.
//!
//! Fixture values are obviously fake (`.agents/rules/tests.md`).

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{
    ApiKeyLocation, Auth, Body, HttpRequest, ItemId, KeyValue, Method, Variable, VariableKind,
};
use console::{ConsoleEntry, ConsoleStore, record_outcome};
use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{AppContext, TestAppContext};
use http_engine::{
    Header, HistoryRecorder, HttpClient, RequestBody, ResolvedRequest, Response, ResponseBody,
    SendInput, Transport, TransportError,
};
use secrets::{MemorySecretStore, SecretKey};
use storage::{HistoryEntry, LocalStore};
use variables::{ScopeKind, ScopeStack};

/// A secret variable's value, holding every character the encoders rewrite.
const SECRET: &str = "tok3n+REDACTED/ex=ample@ \"x\"";
const SECRET_KEY: &str = "formSecret";
/// A Bearer token typed into the Auth tab, not referenced through a variable.
const BEARER_TOKEN: &str = "bearer-REDACTED+example/=";
/// An API key typed into the Auth tab, sent under a header no credential list names.
const API_KEY: &str = "apikey-REDACTED+example/=";
const API_KEY_HEADER: &str = "X-Tenant-Key";
const WORKSPACE: &str = "workspace-under-test";

#[gpui::test]
async fn a_form_encoded_secret_leaves_no_form_of_itself_in_the_console_or_history(
    cx: &mut TestAppContext,
) {
    let request = request(
        vec![key_value("password", &format!("{{{{{SECRET_KEY}}}}}"))],
        Auth::None,
    );
    let (entries, rows) = send_and_collect(cx, request, SECRET).await;
    assert_no_form_of(SECRET, &entries, &rows);
}

#[gpui::test]
async fn a_bearer_token_typed_into_the_auth_tab_is_masked_in_the_console_and_history(
    cx: &mut TestAppContext,
) {
    let request = request(
        Vec::new(),
        Auth::Bearer {
            token: Some(BEARER_TOKEN.to_string()),
            extra: BTreeMap::new(),
        },
    );
    let (entries, rows) = send_and_collect(cx, request, BEARER_TOKEN).await;
    assert_no_form_of(BEARER_TOKEN, &entries, &rows);
}

#[gpui::test]
async fn an_api_key_typed_into_the_auth_tab_under_a_custom_header_is_masked(
    cx: &mut TestAppContext,
) {
    let request = request(
        Vec::new(),
        Auth::ApiKey {
            key: Some(API_KEY_HEADER.to_string()),
            value: Some(API_KEY.to_string()),
            location: Some(ApiKeyLocation::Header),
            extra: BTreeMap::new(),
        },
    );
    let (entries, rows) = send_and_collect(cx, request, API_KEY).await;
    assert_no_form_of(API_KEY, &entries, &rows);
}

#[gpui::test]
async fn an_api_key_typed_into_the_auth_tab_for_the_query_is_masked(cx: &mut TestAppContext) {
    let request = request(
        Vec::new(),
        Auth::ApiKey {
            key: Some("tenant_key".to_string()),
            value: Some(API_KEY.to_string()),
            location: Some(ApiKeyLocation::Query),
            extra: BTreeMap::new(),
        },
    );
    let (entries, rows) = send_and_collect(cx, request, API_KEY).await;
    assert_no_form_of(API_KEY, &entries, &rows);
}

/// Send `request` through the production path, record it in the Console, and return the
/// stored Console entries and history rows. `echoed` is what the echo server also puts in
/// its `Location` header.
async fn send_and_collect(
    cx: &mut TestAppContext,
    request: HttpRequest,
    echoed: &str,
) -> (Vec<ConsoleEntry>, Vec<HistoryEntry>) {
    let mut local = LocalStore::open_in_memory().expect("the local store opens");
    local.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(local));
    let transport = EchoTransport {
        location_secret: echoed.to_string(),
    };
    let client = cx.new(|_| HttpClient::new(Arc::new(transport)));
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let input = SendInput {
        request,
        ancestors: Vec::new(),
        scopes: scopes(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(no_secrets),
    };
    let outcome = client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect("the send succeeds");
    assert_eq!(
        outcome.secrets_used.refused_count(),
        0,
        "every fixture secret must be maskable, or this test proves nothing"
    );
    cx.run_until_parked();

    let entries = cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
        console::init(cx);
        record_outcome(&outcome, cx);
        cx.global::<ConsoleStore>()
            .entries()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert_eq!(entries.len(), 1, "the send lands exactly one Console entry");
    let rows = store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list");
    assert_eq!(rows.len(), 1, "the send is recorded exactly once");
    (entries, rows)
}

/// An echo server: the request's URL, headers and form body back as JSON, and a
/// `Location` naming `location_secret` percent-encoded in both a path segment and a query.
struct EchoTransport {
    location_secret: String,
}

impl Transport for EchoTransport {
    fn send(
        &self,
        request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let mut location = url::Url::parse("https://next.example.test/").expect("a valid URL");
        if let Ok(mut segments) = location.path_segments_mut() {
            segments.push(&self.location_secret);
        }
        location
            .query_pairs_mut()
            .append_pair("next", &self.location_secret);
        async move {
            let form = match &request.body {
                RequestBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                _ => String::new(),
            };
            let decoded_form: BTreeMap<String, String> =
                url::form_urlencoded::parse(form.as_bytes())
                    .into_owned()
                    .collect();
            let headers: BTreeMap<&str, &str> = request
                .headers
                .iter()
                .chain(&request.auto_headers)
                .map(|header| (header.name.as_str(), header.value.as_str()))
                .collect();
            let echo = serde_json::json!({
                "url": request.url.as_str(),
                "headers": headers,
                "form": decoded_form,
                "data": form,
            })
            .to_string();
            let mut response = Response::new(302, "Found", ResponseBody::Bytes(echo.into_bytes()));
            response.headers = vec![
                Header::new("Location", location.as_str()),
                Header::new("Content-Type", "application/json"),
            ];
            Ok(response)
        }
        .boxed()
    }
}

/// Every spelling of `secret` the echo can carry: literal, form-encoded, percent-encoded
/// as a URL path segment, and JSON-escaped — each produced by the real encoder.
fn forms_of(secret: &str) -> Vec<(&'static str, String)> {
    let mut path = url::Url::parse("https://oracle.example.test/").expect("a valid URL");
    if let Ok(mut segments) = path.path_segments_mut() {
        segments.push(secret);
    }
    let path_segment = path.path().trim_start_matches('/').to_string();
    let json = serde_json::to_string(secret).expect("a string serializes");
    let json = json.trim_matches('"').to_string();
    vec![
        ("the literal value", secret.to_string()),
        (
            "the form-encoded value",
            url::form_urlencoded::byte_serialize(secret.as_bytes()).collect(),
        ),
        ("the path-segment-encoded value", path_segment),
        ("the JSON-escaped value", json),
    ]
}

fn assert_no_form_of(secret: &str, entries: &[ConsoleEntry], rows: &[HistoryEntry]) {
    let forms = forms_of(secret);
    for entry in entries {
        for (field, text) in [
            ("url", entry.url.as_str()),
            ("raw_request", entry.raw_request.as_str()),
            ("raw_response", entry.raw_response.as_str()),
        ] {
            assert!(
                !entry.raw_request.is_empty() && !entry.raw_response.is_empty(),
                "the entry records the exchange"
            );
            for (what, form) in &forms {
                assert!(
                    !text.contains(form.as_str()),
                    "the stored Console entry's {field} leaked {what}"
                );
            }
        }
    }
    for row in rows {
        let bytes = row_bytes(row);
        for (what, form) in &forms {
            assert!(
                !bytes
                    .windows(form.len())
                    .any(|window| window == form.as_bytes()),
                "the stored history row leaked {what}"
            );
        }
    }
}

/// One history row as the bytes its sinks hold: target, request JSON, response summary
/// and body prefix.
fn row_bytes(entry: &HistoryEntry) -> Vec<u8> {
    let mut bytes = entry.target.clone().into_bytes();
    bytes.extend_from_slice(entry.request_json.to_string().as_bytes());
    if let Some(summary) = &entry.response_summary_json {
        bytes.extend_from_slice(summary.to_string().as_bytes());
    }
    if let Some(prefix) = &entry.response_body_prefix {
        bytes.extend_from_slice(prefix);
    }
    bytes
}

fn request(fields: Vec<KeyValue>, auth: Auth) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "Send with encoded secrets".to_string(),
        method: Method::Post,
        url: "https://api.example.test/anything".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::UrlEncoded {
            fields,
            extra: BTreeMap::new(),
        },
        auth,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn key_value(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::with_local_values(BTreeMap::from([(
        SECRET_KEY.to_string(),
        SECRET.to_string(),
    )]));
    scopes.push(
        ScopeKind::Environment,
        vec![Variable {
            key: SECRET_KEY.to_string(),
            value: Some(SECRET.to_string()),
            kind: VariableKind::Secret,
            enabled: true,
            description: None,
            extra: BTreeMap::new(),
        }],
    );
    scopes
}

/// No keychain key: the fixture secret carries a local value and is never fetched.
fn no_secrets(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}
