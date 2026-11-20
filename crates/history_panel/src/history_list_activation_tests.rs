//! Row activation (PRD REQ-HIST-02): a click on a history row, or Enter on the keyboard
//! selection, opens that entry through [`crate::open_entry`] — the request tab lands in
//! the center pane. The scenario drives the panel the way the shell does: a workspace
//! window with the panel in its left dock, the same in-memory store path the
//! `history_list_tests` scenario records through, and real keystrokes on the loaded
//! default keymap. A list that renders its rows but never opens one fails it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext};
use request_editor::RequestEditor;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::HistoryList;

/// The workspace every test records and opens under.
const WORKSPACE: &str = "workspace-under-test";

/// The two entries' targets: distinct URLs, so the tab that opens names the row that was
/// activated. Obviously fake (the `history_list_tests` fixture style).
const NEWEST_TARGET: &str = "https://api.test/login";
const OLDEST_TARGET: &str = "https://api.test/users";

/// The one UTC day the two fixtures land on.
const FIXTURE_DAY: (i32, u8) = (2026, 18);

/// A UTC timestamp on the fixture day at `hour`:00 — an hour apart, so the read orders
/// the rows deterministically (newest first).
fn timestamp(hour: u8) -> OffsetDateTime {
    let day = Date::from_calendar_date(FIXTURE_DAY.0, Month::November, FIXTURE_DAY.1)
        .expect("a real calendar date");
    PrimitiveDateTime::new(
        day,
        Time::from_hms(hour, 0, 0).expect("a whole hour is a valid time"),
    )
    .assume_utc()
}

/// The local store the panel runs against: in memory and migrated (the
/// `history_list_tests` fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one send the way `http_engine` records it — the pre-resolution request as
/// `request_json` — so [`crate::open_entry`] can restore it into a request tab.
fn record(store: &Arc<Mutex<LocalStore>>, target: &str, created_at: OffsetDateTime) {
    let request = HttpRequest {
        id: ItemId::new(),
        name: String::from("History fixture"),
        method: Method::Get,
        url: target.to_owned(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    };
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at,
        protocol: String::from("http"),
        method: String::from("GET"),
        target: target.to_owned(),
        status: Some(200),
        duration_ms: Some(142),
        request_json: serde_json::to_value(request).expect("the request serializes"),
        response_summary_json: None,
        response_body_prefix: None,
    };
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&entry)
        .expect("the entry stores");
}

/// Open the panel the way the shell does — in a workspace window, registered in the left
/// dock, with the app's default keymap loaded so keystrokes dispatch like at runtime —
/// and let its first read land.
fn open_history_panel(
    store: Arc<Mutex<LocalStore>>,
    cx: &mut TestAppContext,
) -> (
    Entity<HistoryList>,
    Entity<Workspace>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the editing keys are unbound — and opening an
        // entry builds the request editor's URL bar.
        text_input::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the bundled keymap must bind cleanly: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let panel = cx.update(|window, cx| {
        let panel = cx.new(|cx| HistoryList::new(Arc::clone(&store), WORKSPACE, window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel.clone(), window, cx);
        });
        panel
    });
    draw_window(cx);
    cx.run_until_parked();
    (panel, workspace, cx)
}

/// The URL the center pane's active request editor holds, if the active tab is one.
fn active_request_url(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> String {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .map(|item| item.to_any_view())
                .and_then(|view| view.downcast::<RequestEditor>().ok())
                .expect("the center pane's active item is a request editor")
                .read(cx)
                .draft()
                .url
                .clone()
        })
    })
}

#[gpui::test]
fn enter_on_selected_history_row_opens_request_tab(cx: &mut TestAppContext) {
    let store = open_store();
    record(&store, OLDEST_TARGET, timestamp(10));
    record(&store, NEWEST_TARGET, timestamp(11));

    let (panel, workspace, cx) = open_history_panel(store, cx);

    // Focus the list the way the panel does, then walk the keyboard: Down onto the
    // first entry row (the day header is not a row the selection can land on), Enter to
    // open it.
    let focus_handle = cx.read(|cx| panel.read_with(cx, |panel, cx| panel.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    draw_window(cx);
    cx.run_until_parked();

    let url = active_request_url(&workspace, cx);
    assert_eq!(
        url, NEWEST_TARGET,
        "Enter opens the selected row — the newest entry's request tab (REQ-HIST-02)"
    );
    assert_ne!(
        url, OLDEST_TARGET,
        "and not the row the selection is not on"
    );
}
