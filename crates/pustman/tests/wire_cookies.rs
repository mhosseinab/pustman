//! The app's cookie jar wire (S33p): one jar the app's sends capture `Set-Cookie` into and
//! send `Cookie` back from (REQ-HTTP-08), persisted through the local store (REQ-WSP-06)
//! and listed by the Manage Cookies dialog (issue #286). Every test boots the app the way
//! the binary boots — `pustman::init_with_store` over a store the test shares — and the
//! send test runs through the real `ReqwestTransport` the app builds its client from,
//! against a loopback [`TestServer`].
//!
//! `Decision:` http_engine's `TestServer` cannot answer with a `Set-Cookie` header, and the
//! helper that can (`cookie_test_support`'s `ScriptedServer`) is compiled only into
//! http_engine's own test build, outside this step's edits set. The tests therefore seed
//! the jar through its public `Set-Cookie` entry point — the very call a response's capture
//! makes — and pin the rest end to end: the app's transport sends the cookie back on the
//! wire, the local store keeps it across a relaunch, and the dialog lists it. The capture
//! direction itself is pinned in http_engine's `cookie_transport_tests`, against a server
//! that does emit `Set-Cookie`. Cookie values are secrets (PRD §7), so every fixture value
//! here is obviously fake (`.agents/rules/tests.md`).

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use http_engine::{
    CookieJar, FakeTransport, HttpClient, ReceivedRequest, SendInput, SendOutcome, TestServer,
};
use pustman::{AppCookieJar, StartupProblems, StoreLocation};
use pustman_actions::cookies::Manage;
use pustman_actions::request::Send;
use pustman_actions::workspace::NewRequest;
use request_editor::send::{RequestClient, set_http_client};
use response_viewer::cookie_dialog::CookieDialog;
use secrets::MemorySecretStore;
use storage::LocalStore;
use ui::test_support::draw_window;
use url::Url;
use variables::ScopeStack;
use workspace::Workspace;

/// An obviously fake cookie value (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";

/// The URL the tests send to when no server is needed (the fake transport answers any).
const SEND_URL: &str = "https://example.test/cookies";

/// A `Set-Cookie` the app's jar captured goes back on the next request's wire — the
/// transport applies the shared jar's cookie header to the resolved request it sends
/// (REQ-HTTP-08, REQ-NET-04), which is what the server records arriving.
#[gpui::test]
async fn test_a_set_cookie_response_is_sent_back_on_the_next_request(cx: &mut TestAppContext) {
    // The server and the sends cross real sockets on the app's tokio runtime: the
    // scheduler may park for their wake-ups (the `gpui-testing` skill's real-IO mode).
    cx.executor().allow_parking();
    let store = shared_store();
    let (_workspace, cx) = boot_app(cx, store);
    let server = start_test_server(cx);

    let first = send_get_request(cx, server.url().as_str()).await;
    assert_eq!(
        first.response.status, 200,
        "the first send reaches the server"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1, "the server saw the first request");
    assert_eq!(
        cookie_header(&requests[0]),
        None,
        "no cookie goes out before any Set-Cookie was captured"
    );

    // The `Set-Cookie` the response would have left in the app's jar (Decision above),
    // through the jar's public capture entry point — the same call `cookie_transport`
    // makes for a response header.
    app_cookie_jar(cx)
        .lock()
        .expect("the jar lock is free")
        .store_response_cookies(server.url(), &[format!("sid={SID_VALUE}; Path=/")]);

    let second = send_get_request(cx, server.url().as_str()).await;
    assert_eq!(
        second.response.status, 200,
        "the second send reaches the server"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2, "the server saw both requests");
    let expected = format!("sid={SID_VALUE}");
    assert_eq!(
        cookie_header(&requests[1]),
        Some(expected.as_str()),
        "the captured cookie goes back on the next request's wire (REQ-HTTP-08)"
    );
}

/// The jar a completed send captures into survives quitting the app: the send's Console
/// entry is the app's completed-send hook and saves the jar to the local store, and the
/// next launch loads it back into its own jar (REQ-WSP-06).
#[gpui::test]
fn test_the_cookie_jar_survives_a_restart_through_the_local_store(cx: &mut TestAppContext) {
    let store = shared_store();
    let (_workspace, cx) = boot_app(cx, Arc::clone(&store));
    // The client tests send through: a completed send is what triggers the save, and the
    // transport plays no part in the persistence (the `wire_m1_fixes.rs` shape).
    let transport = FakeTransport::with_response(cx.executor(), 200, b"saved".to_vec());
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|_window, cx| set_http_client(client, cx));

    // The cookie a send captured into the app's jar (the module docs' `Decision:`).
    app_cookie_jar(cx)
        .lock()
        .expect("the jar lock is free")
        .store_response_cookies(
            &Url::parse("https://example.test/").expect("the fixture URL parses"),
            &[format!("sid={SID_VALUE}; Path=/")],
        );

    // A completed send, the way a user makes one: a request tab's Send. Its Console entry
    // is what the save wire listens for (REQ-WSP-06).
    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    cx.simulate_input(SEND_URL);
    cx.dispatch_action(Send);
    cx.run_until_parked();

    // The relaunch: a fresh init over the same database (the `wire_restore.rs` shape).
    cx.update(|_window, cx| {
        pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store)))
    });
    cx.run_until_parked();

    let entries = app_cookie_jar(cx)
        .lock()
        .expect("the jar lock is free")
        .list();
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.as_str()))
            .collect::<Vec<_>>(),
        vec![("sid", SID_VALUE)],
        "the cookie a send captured survives a restart through the local store (REQ-WSP-06)"
    );
}

/// Dispatching `cookies::Manage` opens the Manage Cookies dialog over the app's jar: the
/// cookie the server set is listed without any re-wiring (issue #286).
#[gpui::test]
fn test_manage_cookies_opens_the_dialog_over_the_apps_jar(cx: &mut TestAppContext) {
    let store = shared_store();
    let (workspace, cx) = boot_app(cx, store);

    // The cookie a send captured into the app's jar (the module docs' `Decision:`).
    app_cookie_jar(cx)
        .lock()
        .expect("the jar lock is free")
        .store_response_cookies(
            &Url::parse("https://example.test/").expect("the fixture URL parses"),
            &[format!("sid={SID_VALUE}; Path=/")],
        );

    cx.dispatch_action(Manage);
    cx.run_until_parked();
    draw_window(cx);

    let dialog = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace.modal_layer().active_modal::<CookieDialog>()
            })
        })
        .expect("dispatching cookies::Manage opens the dialog (issue #286)");
    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    let shown: Vec<(&str, &str, &str)> = rows
        .iter()
        .filter(|row| !row.group_header)
        .map(|row| (row.name.as_str(), row.value.as_str(), row.path.as_str()))
        .collect();
    assert_eq!(
        shown,
        vec![("sid", SID_VALUE, "/")],
        "the dialog lists the cookie the server set, over the app's own jar"
    );
}

/// The app's one window on the full init order over `store`, drawn and active (the
/// `wire_m1_fixes.rs` shape), with the store's background open — and so the cookie wire's
/// jar load — drained before the test goes on.
fn boot_app(
    cx: &mut TestAppContext,
    store: Arc<Mutex<LocalStore>>,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(store)));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

/// The app-wide cookie jar the transport sends and captures through: the same `Arc` the
/// store-open path persists and the Manage Cookies dialog lists.
fn app_cookie_jar(cx: &VisualTestContext) -> Arc<Mutex<CookieJar>> {
    cx.read(|cx| {
        cx.try_global::<AppCookieJar>()
            .expect("the app installed its cookie jar")
            .0
            .clone()
    })
}

/// A loopback server on the app's tokio runtime — the one `gpui_tokio::init` built.
fn start_test_server(cx: &VisualTestContext) -> TestServer {
    let handle = cx.read(gpui_tokio::Tokio::handle);
    handle
        .block_on(TestServer::start())
        .expect("the loopback test server starts")
}

/// Send one GET to `url` through the app's HTTP client and return the outcome.
async fn send_get_request(cx: &mut VisualTestContext, url: &str) -> SendOutcome {
    let client = cx.read(|cx| {
        cx.try_global::<RequestClient>()
            .expect("the app installed its HTTP client")
            .0
            .clone()
    });
    let outcome = client.update(cx, |client, cx| client.send_request(send_input(url), cx));
    outcome.await.expect("the send succeeds")
}

/// The `Cookie` header one recorded request carried, if any.
fn cookie_header(request: &ReceivedRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("cookie"))
        .map(|header| header.value.as_str())
}

/// One GET for a send: empty scopes, a `MemorySecretStore`, and a key lookup that finds
/// no keychain key — nothing here references a secret (the `wire_history_recording.rs`
/// shape).
fn send_input(url: &str) -> SendInput {
    SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "cookie wire test".to_string(),
            method: Method::Get,
            url: url.to_string(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        },
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_, _| None),
    }
}

/// The in-memory local store the app and the test share (the `wire_history_recording.rs`
/// shape).
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}
