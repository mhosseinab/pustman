//! The list's accessibility contract (#158): every row is a `Role::ListBoxOption` with
//! its selection state, so the container that draws them must be the `Role::ListBox`
//! they belong to — assistive tech navigates a list by its list, never by loose options.

use std::sync::{Arc, Mutex};

use gpui::{
    Div, Element, ElementId, Entity, IntoElement, Role, Stateful, TestAppContext,
    VisualTestContext, div,
};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::VirtualList;
use ui::test_support::{draw_window, init_test};

use super::HistoryList;

/// The workspace every recorded row belongs to.
const WORKSPACE: &str = "workspace-under-test";

/// The one UTC day the fixture row lands on.
fn timestamp() -> OffsetDateTime {
    let day = Date::from_calendar_date(2026, Month::November, 18).expect("a real calendar date");
    PrimitiveDateTime::new(
        day,
        Time::from_hms(9, 0, 0).expect("a whole hour is a valid time"),
    )
    .assume_utc()
}

/// The in-memory store the panel reads (the `history_list_tests` fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one fixture send and return its stored id.
fn record(store: &Arc<Mutex<LocalStore>>) -> i64 {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at: timestamp(),
        protocol: "http".to_owned(),
        method: "GET".to_owned(),
        target: "https://api.test/users".to_owned(),
        status: Some(200),
        duration_ms: Some(142),
        request_json: json!({ "method": "GET", "url": "https://api.test/users" }),
        response_summary_json: None,
        response_body_prefix: None,
    };
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&entry)
        .expect("the entry stores")
}

/// Open the panel over `store` and let its first read land.
fn open_history_panel(
    store: Arc<Mutex<LocalStore>>,
    cx: &mut TestAppContext,
) -> (Entity<HistoryList>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
    });
    let (panel, cx) =
        cx.add_window_view(|window, cx| HistoryList::new(store, WORKSPACE, window, cx));
    draw_window(cx);
    cx.run_until_parked();
    (panel, cx)
}

/// An empty stand-in for the rows list: the assertion is about the container the panel
/// builds around whatever list it draws, not about the rows themselves.
fn stand_in_list() -> VirtualList {
    VirtualList::new(
        "role-test-rows",
        0,
        |index| ElementId::from(format!("role-test-row-{index}")),
        |_, _, _| div().into_any_element(),
    )
}

#[gpui::test]
fn the_rows_container_is_the_list_box_they_belong_to(cx: &mut TestAppContext) {
    let store = open_store();
    record(&store);
    let (panel, cx) = open_history_panel(store, cx);

    // The role assistive tech would read off the container `body` draws the rows in.
    let role = cx.read(|cx| {
        panel.read_with(cx, |panel, cx| {
            let mut area = panel.body(stand_in_list(), false, &None, false, cx);
            match area.downcast_mut::<Stateful<Div>>() {
                Some(container) => container.a11y_role(),
                None => None,
            }
        })
    });
    assert_eq!(
        role,
        Some(Role::ListBox),
        "the container that draws the ListBoxOption rows must be their ListBox (#158)"
    );
}
