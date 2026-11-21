//! The history-recording wire (S33w): the app's one HTTP client records every send it
//! makes into the local store (PRD §8.5, REQ-HIST-01). Nothing calls
//! `HttpClient::set_history` without this wire, so no send is ever recorded and the S38
//! "History open and restore" hand check has nothing to show. The client here is the
//! `FakeTransport` one tests install (send.rs's `Decision:`), and the store is shared
//! with the test, so `storage`'s history repo can read the row back — no hand-rolled SQL.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, TestAppContext};
use http_engine::{FakeTransport, HttpClient, SendInput};
use pustman::{StartupProblems, StoreLocation};
use request_editor::send::set_http_client;
use secrets::MemorySecretStore;
use storage::{HistoryEntry, LocalStore};
use variables::ScopeStack;
use workspace::Workspace;

/// The workspace the app records under: the same id the History panel reads
/// (`pustman.rs`'s `WORKSPACE_ID`).
const WORKSPACE_ID: &str = "default";

/// The URL the test sends to and the stored row must carry.
const URL: &str = "https://example.test/recorded";

#[gpui::test]
fn wire_history_recording_records_a_sent_request(cx: &mut TestAppContext) {
    // One store shared by the app and the test, so the row can be read back through
    // `storage` after the send (the "shared store" of the wire-test shape).
    let store = shared_store();
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));

    // The `FakeTransport` client tests install, in place before the background store
    // open lands: the wire installs the recorder on whichever client the `RequestClient`
    // global holds at that moment.
    let transport = FakeTransport::with_response(cx.executor(), 200, b"recorded".to_vec());
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|cx| set_http_client(client.clone(), cx));

    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task; the wire gives the client its recorder once
    // both the store and the client exist.
    cx.run_until_parked();

    let input = send_input();
    // Dropping the outcome task does not cancel the send (the client owns it); the row
    // is written inside the client's own work before the outcome resolves.
    let _outcome = client.update(cx, |client, cx| client.send_request(input, cx));
    cx.run_until_parked();

    let rows = stored_rows(&store);
    assert_eq!(
        rows.len(),
        1,
        "the sent request lands one history row through the wired recorder (REQ-HIST-01)"
    );
    assert_eq!(
        rows[0].method,
        Method::Get.as_str(),
        "the row carries the sent method"
    );
    assert_eq!(rows[0].target, URL, "the row carries the sent URL");
}

/// The in-memory local store both the app and the test read and write through.
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// One GET for the client to send and the wire to record: empty scopes, a
/// `MemorySecretStore`, and a key lookup that finds no keychain key — nothing here
/// references a secret.
fn send_input() -> SendInput {
    SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "history recording wire test".to_string(),
            method: Method::Get,
            url: URL.to_string(),
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

/// What `storage`'s history repo holds for the app's workspace, read back through the
/// existing storage API.
fn stored_rows(store: &Mutex<LocalStore>) -> Vec<HistoryEntry> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE_ID, "")
        .expect("the rows list")
}
