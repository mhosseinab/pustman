//! Opening a history entry restores the stored request exactly as it was recorded —
//! `{{var}}` templates intact (PRD REQ-HIST-02). The scenario records a send the way
//! `http_engine` records it (the PRE-resolution request as `request_json`), reads the
//! row back through the store so the open path only ever sees what the database holds,
//! and then opens it through the public open path. An open that resolves the variables,
//! opens the wrong tab, opens a second tab, or drops the recorded response fails it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{Auth, Body, HttpRequest, ItemId, KeyValue, Method};
use gpui::TestAppContext;
use http_engine::ResponseBody;
use request_editor::RequestEditor;
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::OffsetDateTime;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::open_entry;

/// The workspace every test records and opens under.
const WORKSPACE: &str = "workspace-under-test";

/// The pre-resolution request `http_engine` would have recorded: the URL and the auth
/// header still carry their `{{var}}` templates (the fixtures are obviously fake).
fn recorded_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: String::from("List users"),
        method: Method::Get,
        url: String::from("https://{{base}}/users"),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![KeyValue {
            key: String::from("Authorization"),
            value: Some(String::from("Bearer {{token}}")),
            description: None,
            enabled: true,
            extra: BTreeMap::new(),
        }],
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// The local store the scenario records through: in memory and migrated (the
/// `history_list_tests` fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one send — the pre-resolution request as `request_json`, the response summary
/// `http_engine` writes — and read the row back through the store: the stored document
/// is the only thing [`open_entry`] gets.
fn record(store: &Arc<Mutex<LocalStore>>) -> HistoryEntry {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at: OffsetDateTime::now_utc(),
        protocol: String::from("http"),
        method: String::from("GET"),
        target: String::from("https://{{base}}/users"),
        status: Some(200),
        duration_ms: Some(142),
        request_json: serde_json::to_value(recorded_request()).expect("the request serializes"),
        response_summary_json: Some(json!({
            "status": 200,
            "timings": { "total_ms": 142 },
            "body_size": 2,
            "headers": [{ "name": "content-type", "value": "application/json" }],
        })),
        response_body_prefix: Some(b"[]".to_vec()),
    };
    let stored_id = store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&entry)
        .expect("the entry stores");
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the history reads back")
        .into_iter()
        .find(|entry| entry.id == Some(stored_id))
        .expect("the stored entry reads back")
}

#[gpui::test]
fn open_entry_restores_unresolved_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the editing keys are unbound — and opening an
        // entry builds the request editor's URL bar.
        text_input::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);

    let entry = record(&open_store());

    cx.update(|window, cx| open_entry(&entry, &workspace, window, cx));
    draw_window(cx);
    cx.run_until_parked();

    // The new tab is a request editor whose draft still carries the literal templates:
    // the stored request is opened UNRESOLVED (REQ-HIST-02).
    let (url, authorization) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            let editor = pane
                .active_item()
                .map(|item| item.to_any_view())
                .and_then(|view| view.downcast::<RequestEditor>().ok())
                .expect("the opened tab is a request editor");
            let draft = editor.read(cx).draft();
            (
                draft.url.clone(),
                draft
                    .headers
                    .iter()
                    .find(|header| header.key == "Authorization")
                    .and_then(|header| header.value.clone()),
            )
        })
    });
    assert_eq!(
        url, "https://{{base}}/users",
        "the draft still carries the literal `{{base}}` — nothing is resolved on open"
    );
    assert_eq!(
        authorization.as_deref(),
        Some("Bearer {{token}}"),
        "and the header still carries the literal `{{token}}`"
    );

    // One tab only, and the recorded response shows in that tab's own response region —
    // a second "summary" tab beside it would repeat what the region already shows.
    let (tab_count, response) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            let editor = pane
                .active_item()
                .map(|item| item.to_any_view())
                .and_then(|view| view.downcast::<RequestEditor>().ok())
                .expect("the opened tab is a request editor");
            let response = editor.read(cx).response_view().read(cx).response().cloned();
            (pane.item_ids().len(), response)
        })
    });
    assert_eq!(tab_count, 1, "opening an entry opens exactly one tab");
    let response = response.expect("the recorded response shows in the request tab");
    assert_eq!(response.status, 200, "with the recorded status");
    assert_eq!(
        response.reason, "OK",
        "and the status's standard reason phrase"
    );
    assert_eq!(
        response.timings.total,
        Some(Duration::from_millis(142)),
        "and the recorded total time"
    );
    assert_eq!(
        response
            .headers
            .iter()
            .map(|header| (header.name.as_str(), header.value.as_str()))
            .collect::<Vec<_>>(),
        vec![("content-type", "application/json")],
        "and the recorded headers"
    );
    match &response.body {
        ResponseBody::Bytes(bytes) => assert_eq!(bytes.as_slice(), b"[]", "and the stored body"),
        other => panic!("the stored body is shown as bytes, got {other:?}"),
    }
}

#[gpui::test]
fn open_entry_of_a_failed_send_opens_only_the_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);

    let mut entry = record(&open_store());
    entry.response_summary_json = Some(json!({ "error": "connect" }));
    entry.response_body_prefix = None;

    cx.update(|window, cx| open_entry(&entry, &workspace, window, cx));
    draw_window(cx);
    cx.run_until_parked();

    let (tab_count, response) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            let editor = pane
                .active_item()
                .map(|item| item.to_any_view())
                .and_then(|view| view.downcast::<RequestEditor>().ok())
                .expect("the opened tab is a request editor");
            let response = editor.read(cx).response_view().read(cx).response().cloned();
            (pane.item_ids().len(), response)
        })
    });
    assert_eq!(tab_count, 1);
    assert!(
        response.is_none(),
        "a send that never got a response shows none — nothing is made up"
    );
}
