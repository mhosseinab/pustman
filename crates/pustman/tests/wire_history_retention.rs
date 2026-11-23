//! The history recorder follows `history_retention_days` while the app runs (#196): the
//! window was read once, when the recorder was built, so a settings change waited for a
//! restart. The change here is made the way a user makes one — a fresh settings blob
//! through `SettingsStore::reload`, as `console`'s cap test does — and observed through
//! the one effect the window has: which rows the session's single retention prune
//! removes (REQ-HIST-04).

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
use time::{Duration, OffsetDateTime};
use variables::ScopeStack;
use workspace::Workspace;

/// The workspace the app records under: the same id the History panel reads
/// (`pustman.rs`'s `WORKSPACE_ID`).
const WORKSPACE_ID: &str = "default";

/// The URL the test sends to and the fresh row carries.
const URL: &str = "https://example.test/retained";

/// The URL of the seeded row, recorded before the app ran — 40 days old, inside the
/// default 30-day window's past.
const OLD_URL: &str = "https://example.test/past-the-window";

#[gpui::test]
fn history_retention_days_change_reaches_the_recorder(cx: &mut TestAppContext) {
    // One store shared by the app and the test, seeded with a row from 40 days ago: the
    // default 30-day window's prune deletes it, a disabled one keeps it.
    let store = shared_store();
    seed_old_row(&store);
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));

    let transport = FakeTransport::with_response(cx.executor(), 200, b"retained".to_vec());
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|cx| set_http_client(client.clone(), cx));

    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task; the wire installs the recorder once both the
    // store and the client exist.
    cx.run_until_parked();

    // The user turns retention off while the app runs (`0` disables it, as in
    // `storage`'s `prune_older_than`) — the recorder must prune by the new window, not
    // by the one it was built with (issue #196).
    cx.update(|_window, cx| {
        let reload = settings::SettingsStore::reload(cx, Some(r#"{"history_retention_days": 0}"#));
        assert_eq!(
            reload,
            Ok(()),
            "history_retention_days is a setting the user can change"
        );
    });
    cx.run_until_parked();

    // The first send after the change writes one row and runs the session's one prune —
    // by whatever retention window the recorder holds now.
    let _outcome = client.update(cx, |client, cx| client.send_request(send_input(), cx));
    cx.run_until_parked();

    let targets = stored_targets(&store);
    assert!(
        targets.iter().any(|target| target == OLD_URL),
        "a history_retention_days change reaches the recorder at once: with retention \
         off the prune keeps the 40-day-old row (REQ-HIST-04, issue #196); rows: {targets:?}"
    );
}

/// The in-memory local store both the app and the test read and write through.
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// One history row from 40 days ago, written straight through `storage` — the recorder
/// only prunes rows that predate the app, so this is what a real install starts with.
fn seed_old_row(store: &Mutex<LocalStore>) {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE_ID.to_owned(),
        created_at: OffsetDateTime::now_utc() - Duration::days(40),
        protocol: "http".to_owned(),
        method: Method::Get.as_str().to_owned(),
        target: OLD_URL.to_owned(),
        status: Some(200),
        duration_ms: Some(1),
        request_json: serde_json::json!({}),
        response_summary_json: None,
        response_body_prefix: None,
    };
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&entry)
        .expect("the seeded row inserts");
}

/// One GET for the client to send and the wire to record: empty scopes, a
/// `MemorySecretStore`, and a key lookup that finds no keychain key — nothing here
/// references a secret.
fn send_input() -> SendInput {
    SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "history retention wire test".to_string(),
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

/// The stored rows' targets, for the one assertion this file makes.
fn stored_targets(store: &Mutex<LocalStore>) -> Vec<String> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE_ID, "")
        .expect("the rows list")
        .into_iter()
        .map(|entry| entry.target)
        .collect()
}
