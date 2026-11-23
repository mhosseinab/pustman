//! The list's focus ring must never move its layout (#157): the border around the rows
//! is drawn whether or not the panel holds keyboard focus — transparent unfocused, the
//! theme's focus colour focused — so moving focus onto the panel leaves every row
//! exactly where it was. A border drawn only on focus shifts the whole list 1px on every
//! focus change.

use std::sync::{Arc, Mutex};

use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};

use super::{HistoryList, target_debug_selector};

/// The workspace every recorded row belongs to.
const WORKSPACE: &str = "workspace-under-test";

/// The row the fixture's one entry draws as: under its day header, second. Its stored id
/// is what the target's selector derives from.
const ENTRY_ROW_INDEX: usize = 1;

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

/// `debug_bounds` keys on `'static` strings while the target's selector is built at run
/// time — the `history_row_tests` leak shape.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

#[gpui::test]
fn moving_focus_onto_the_panel_never_shifts_the_rows(cx: &mut TestAppContext) {
    let store = open_store();
    let id = record(&store);
    let (panel, cx) = open_history_panel(store, cx);
    let selector = leak(target_debug_selector(Some(id), ENTRY_ROW_INDEX));

    draw_window(cx);
    let unfocused = cx
        .debug_bounds(selector)
        .expect("the row's target renders while nothing is focused");

    // Move keyboard focus onto the panel — the moment its focus ring appears.
    let focus_handle = cx.read(|cx| panel.read_with(cx, |panel, cx| panel.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    let focused = cx
        .debug_bounds(selector)
        .expect("the row's target renders while the panel is focused");

    assert_eq!(
        focused, unfocused,
        "the focus border is always drawn (transparent while unfocused), so moving focus \
         onto the panel must not shift the rows' layout (#157)"
    );
}
