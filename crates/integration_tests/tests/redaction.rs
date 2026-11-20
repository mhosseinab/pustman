//! The redaction NFR end to end (REQ-NFR-04, REQ-HIST-03, REQ-CON-05/06, REQ-WSP-06):
//! one send whose secret value is used in the URL query, a header, the Basic password, a
//! Bearer token and the JSON body must leave that value in none of the sinks no single
//! crate's test can see — every workspace file, the SQLite file's bytes, the stored
//! history rows, and the Console export text. The send goes through the production path
//! (`HttpClient::send_request` → `prepare` → history → `console::record_outcome`), with a
//! `FakeTransport` answering an echo of the request — the response side every redaction
//! must survive — and the response's cookie landing in the jar.
//!
//! **Cookie-jar Decision** (binding, plan § S37): the jar is local state per REQ-WSP-06
//! and is NOT redacted. The response's `Set-Cookie` value equals the secret and persists
//! verbatim through `cookie_persistence::save_jar`; this test pins that exemption
//! positively (the row is written, with its value) and asserts the value is absent from
//! every other sink — history rows and workspace files. The SQLite byte scan runs after
//! the history write and BEFORE the jar is persisted, so the exempt cookie rows cannot
//! stand in for a history leak.
//!
//! Fixture values are obviously fake (`.agents/rules/tests.md`): `S3cr3t-Value-123` is
//! leetspeak, not a credential — spelled exactly as the plan binds it.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helper fns below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, KeyValue, Method, RawLanguage, Request, Variable,
    VariableKind, Workspace,
};
use base64::Engine;
use console::{ConsoleEntry, ConsoleStore, export_text, record_outcome};
use gpui::{AppContext, TestAppContext};
use http_engine::{CookieJar, FakeTransport, HistoryRecorder, HttpClient, SendInput, save_jar};
use secrets::{MemorySecretStore, SecretKey};
use storage::{HistoryEntry, LocalStore};
use variables::{ScopeKind, ScopeStack};
use workspace_store::{FORMAT_VERSION, Item, ItemBody};

/// The secret's local value (the plan's fixture). Obviously fake: leetspeak, and no
/// scanner's pattern (`.agents/rules/tests.md`).
const SECRET: &str = "S3cr3t-Value-123";
/// The `{{name}}` the fixture request references the secret by.
const SECRET_KEY: &str = "secretToken";
/// The Basic username: together with [`SECRET`] it fixes the derived credential form
/// (`base64(user:pass)`) the test also hunts for.
const USERNAME: &str = "user";
/// One workspace id, as the app derives it from the workspace's identity.
const WORKSPACE: &str = "workspace-under-test";

#[gpui::test]
async fn no_secret_in_workspace_files_sqlite_history_or_console_export(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().expect("the scratch directory opens");
    // The local store is per-user state, not workspace content: outside the workspace
    // dir, so the workspace walk below reads exactly what `save_item` wrote.
    let workspace_root = scratch.path().join("workspace");
    let database_path = scratch.path().join("local-store.sqlite");
    save_fixture_workspace(&workspace_root);
    let snapshot = workspace_store::open(&workspace_root).expect("the workspace opens");
    assert!(
        snapshot.problems.is_empty(),
        "the fixture workspace reopens cleanly: {:?}",
        snapshot.problems
    );
    assert_eq!(
        snapshot.items.len(),
        3,
        "workspace, environment and request"
    );

    let store = open_file_store(&database_path);
    let echo = echo_body(&basic_credential());
    let transport = FakeTransport::with_response(cx.executor(), 200, echo);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });

    let outcome = client
        .update(cx, |client, cx| client.send_request(send_input(), cx))
        .await
        .expect("the send succeeds");
    assert_eq!(
        outcome.secrets_used.refused_count(),
        0,
        "the fixture secret must be maskable, or this test proves nothing"
    );

    // Sink 2 (first half): the SQLite file as the history write left it, before the
    // exempt cookie rows can hide a leak.
    let database_bytes = std::fs::read(&database_path).expect("the database file reads");
    assert_no_secret(&database_bytes, "the SQLite file after the history write");

    // The response's cookie, whose value is the secret. Cookies are local state
    // (REQ-WSP-06): the jar keeps the value verbatim and `save_jar` persists it — the
    // exemption recorded in this file's doc comment.
    let mut jar = CookieJar::new();
    jar.store_response_cookies(&example_url(), &[format!("session={SECRET}; Path=/")]);
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");

    // Sink 2 (second half): the history rows the send stored, read back through
    // `HistoryRepo`, are still clean after the jar is persisted beside them.
    let rows = {
        let store = store.lock().expect("the lock is free");
        store
            .history()
            .search(WORKSPACE, "")
            .expect("the rows list")
    };
    assert_eq!(rows.len(), 1, "the send is recorded exactly once");
    for row in &rows {
        assert_no_secret(&row_bytes(row), "the stored history row");
    }
    // The exempt sink is pinned as exempt, not merely absent: the jar row carries the
    // cookie verbatim, which is what REQ-WSP-06 allows and the walk below never sees.
    let cookie_rows = {
        let store = store.lock().expect("the lock is free");
        store.cookies().list(WORKSPACE).expect("the rows list")
    };
    assert_eq!(
        cookie_rows.len(),
        1,
        "the jar holds the one response cookie"
    );
    assert_eq!(
        cookie_rows[0].value, SECRET,
        "the cookie value is local state and stays as it is (REQ-WSP-06)"
    );

    // Sink 3: the Console entry the real feed records, exported as text.
    let exported = cx.update(|cx| {
        init_console(cx);
        record_outcome(&outcome, cx);
        let entries: Vec<ConsoleEntry> = cx
            .global::<ConsoleStore>()
            .entries()
            .iter()
            .cloned()
            .collect();
        assert_eq!(entries.len(), 1, "the send lands exactly one Console entry");
        export_text(&entries, &outcome.secrets_used)
    });
    assert!(!exported.is_empty(), "the export renders the entry");
    assert_no_secret(exported.as_bytes(), "the Console export text");

    // Sink 1: every workspace file, walked last so nothing written later can escape it.
    for (path, bytes) in read_all_files(&workspace_root) {
        assert_no_secret(&bytes, &format!("the workspace file {}", path.display()));
    }
}

/// The saved environment file's secret variable carries only `key`, `kind` and
/// `description` — no `value` field at all — so a workspace file can never hold a
/// secret, shared or local (REQ-VAR-SHARE-02, REQ-WSP-06).
#[test]
fn secret_variable_file_has_key_kind_description_only() {
    let scratch = tempfile::tempdir().expect("the scratch directory opens");
    let environment = Item {
        body: ItemBody::Environment(Environment {
            id: ItemId::new(),
            name: "Local".to_string(),
            variables: vec![secret_variable()],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("environments"),
        path: None,
    };

    let saved =
        workspace_store::save_item(scratch.path(), &environment).expect("the environment saves");
    let bytes = std::fs::read(scratch.path().join(saved)).expect("the file reads");

    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("the file is JSON");
    let variable = document
        .get("variables")
        .and_then(|variables| variables.get(0))
        .and_then(serde_json::Value::as_object)
        .expect("the file holds the variable object");
    let mut keys: Vec<&str> = variable.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["description", "key", "kind"],
        "a secret variable keeps only key, kind and description"
    );
    assert!(
        variable.get("value").is_none(),
        "a secret variable has no value field at all"
    );
    assert!(
        !contains(&bytes, SECRET.as_bytes()),
        "the saved file must not carry the secret value"
    );
}

/// The fixture request: the secret used in the URL query, a header, the Basic password,
/// a Bearer token and the JSON body — the five places the plan names. The request's own
/// `Authorization: Bearer {{secretToken}}` header is the Bearer-token use site;
/// `request_auth::set_header` replaces its value with the Basic credential before the
/// wire (one `Authorization` per request), so the resolved bearer text reaches no record
/// while the resolved secret still reaches every sink through the query, the
/// `X-Api-Key` header, the body and the echoed base64 credential.
fn fixture_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "Send with the secret".to_string(),
        method: Method::Post,
        url: "https://api.example.test/users?token={{secretToken}}".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![
            key_value("X-Api-Key", "{{secretToken}}"),
            key_value("Authorization", "Bearer {{secretToken}}"),
        ],
        body: Body::Raw {
            language: RawLanguage::Json,
            text: "{\"password\": \"{{secretToken}}\"}".to_string(),
            extra: BTreeMap::new(),
        },
        auth: Auth::Basic {
            username: Some(USERNAME.to_string()),
            password: Some("{{secretToken}}".to_string()),
            extra: BTreeMap::new(),
        },
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// One enabled header row.
fn key_value(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// The environment's one variable: a secret with a local value (REQ-VAR-SHARE-01).
/// `VariableKind::Secret` never serializes its value — the file shape
/// `secret_variable_file_has_key_kind_description_only` pins.
fn secret_variable() -> Variable {
    Variable {
        key: SECRET_KEY.to_string(),
        value: Some(SECRET.to_string()),
        kind: VariableKind::Secret,
        enabled: true,
        description: Some("the fixture's secret".to_string()),
        extra: BTreeMap::new(),
    }
}

/// The fixture workspace: `pustman.json`, the environment carrying the secret variable,
/// and the request that uses it in five places.
fn save_fixture_workspace(root: &Path) {
    let items = [
        Item {
            body: ItemBody::Workspace(Workspace {
                id: ItemId::new(),
                name: "Redaction fixture".to_string(),
                format_version: FORMAT_VERSION.to_string(),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::new(),
            path: None,
        },
        Item {
            body: ItemBody::Environment(Environment {
                id: ItemId::new(),
                name: "Local".to_string(),
                variables: vec![secret_variable()],
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from("environments"),
            path: None,
        },
        Item {
            body: ItemBody::Request(Request::Http(fixture_request())),
            dir: PathBuf::from("collections/users-api"),
            path: None,
        },
    ];
    for item in &items {
        workspace_store::save_item(root, item).expect("the item saves");
    }
}

/// Everything [`HttpClient::send_request`] needs for the fixture send.
fn send_input() -> SendInput {
    SendInput {
        request: fixture_request(),
        ancestors: Vec::new(),
        scopes: scopes(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(no_secrets),
    }
}

/// The scopes the send resolves against: the secret variable's local value wins
/// (REQ-VAR-SHARE-01), so the keychain store is never read.
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::with_local_values(BTreeMap::from([(
        SECRET_KEY.to_string(),
        SECRET.to_string(),
    )]));
    scopes.push(ScopeKind::Environment, vec![secret_variable()]);
    scopes
}

/// No keychain key: this fixture's secret carries a local value and is never fetched.
fn no_secrets(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}

/// The in-memory `MemorySecretStore`'s file-backed counterpart: the per-user local store
/// on disk, migrated — the test reads its bytes.
fn open_file_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The echo an echo server (httpbin `/anything`) answers with: the resolved request's
/// URL, headers and body back at the caller — the response-side leak vector every sink's
/// redaction must survive. The Basic credential arrives as its derived base64 form, as
/// an echoing server returns it.
fn echo_body(encoded: &str) -> String {
    serde_json::json!({
        "url": format!("https://api.example.test/users?token={SECRET}"),
        "headers": {
            "X-Api-Key": SECRET,
            "Authorization": format!("Basic {encoded}"),
        },
        "body": format!("{{\"password\": \"{SECRET}\"}}"),
    })
    .to_string()
}

/// The `Authorization` value the Basic auth puts on the wire — computed here as an
/// independent oracle over the `base64` crate, not read back from the send.
fn basic_credential() -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("{USERNAME}:{SECRET}"))
}

/// The URL the response set its cookie for: the request's host.
fn example_url() -> url::Url {
    url::Url::parse("https://api.example.test/users").expect("a valid URL")
}

/// One history row as the bytes its sinks hold: the redacted target, the request JSON,
/// the response summary and the body prefix.
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

/// Every file under `root`, recursively, as (path relative to `root`, bytes).
fn read_all_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files
}

/// Append every file under `dir` to `files`, descending into subdirectories.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("the directory reads") {
        let entry = entry.expect("the directory entry reads");
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("the file is under the root")
                .to_path_buf();
            files.push((relative, std::fs::read(&path).expect("the file reads")));
        }
    }
}

/// Assert that neither the secret value nor its derived base64 Basic credential appears
/// in `haystack`, naming `sink` — which sink leaked — in every failure message.
fn assert_no_secret(haystack: &[u8], sink: &str) {
    let encoded = basic_credential();
    for (what, needle) in [
        ("the secret value", SECRET.as_bytes()),
        ("the base64 Basic credential", encoded.as_bytes()),
    ] {
        assert!(!contains(haystack, needle), "{sink} leaked {what}");
    }
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The Console's store, installed the way `main.rs` installs it: settings first.
fn init_console(cx: &mut gpui::App) {
    let init = settings::SettingsStore::init(cx, None);
    assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
    console::init(cx);
}
