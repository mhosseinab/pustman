//! What one history row says about the send it shows (#235): a send that got no status
//! code names its state — "Failed" or "Cancelled" — in the row's meta line, never a
//! bare '—', and the target's hover tooltip is a truncation tooltip and nothing else: a
//! URL drawn whole is its own tooltip, and only one the row truncates carries the full
//! value (the `response_viewer` body line's contract, #268).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{Entity, Modifiers, MouseButton, Point, TestAppContext, VisualTestContext, px};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::components::tooltip::TOOLTIP_SELECTOR;
use ui::test_support::{draw_window, init_test};

use super::{HistoryList, entry_summary, target_debug_selector};

/// The workspace every recorded row belongs to.
const WORKSPACE: &str = "workspace-under-test";

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// How far into the target's box the tests rest the pointer — well within any target the
/// fixtures draw, short or truncated.
const HOVER_INTO_TARGET_PX: f32 = 8.;

/// The one UTC day every fixture row lands on.
fn timestamp() -> OffsetDateTime {
    let day = Date::from_calendar_date(2026, Month::November, 18).expect("a real calendar date");
    PrimitiveDateTime::new(
        day,
        Time::from_hms(9, 0, 0).expect("a whole hour is a valid time"),
    )
    .assume_utc()
}

/// One recorded send: a response row carries its status and timing, while a failed or
/// cancelled one carries the summary marker `http_engine` records for it and no status.
fn entry(target: &str, status: Option<i64>, summary: Option<serde_json::Value>) -> HistoryEntry {
    HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at: timestamp(),
        protocol: "http".to_owned(),
        method: "GET".to_owned(),
        target: target.to_owned(),
        status,
        duration_ms: status.map(|_| 142),
        request_json: json!({ "method": "GET", "url": target }),
        response_summary_json: summary,
        response_body_prefix: None,
    }
}

/// The in-memory store the panel reads (the `history_list_tests` fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one fixture send and return its stored id.
fn record(store: &Arc<Mutex<LocalStore>>, entry: &HistoryEntry) -> i64 {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(entry)
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

/// Rest the pointer on the target of the row `id` draws until a tooltip would show;
/// report whether one did.
fn tooltip_after_hovering_target(id: i64, cx: &mut VisualTestContext) -> bool {
    draw_window(cx);
    let target = cx
        .debug_bounds(leak(target_debug_selector(Some(id))))
        .expect("the target rendered");
    let hover = Point::new(target.left() + px(HOVER_INTO_TARGET_PX), target.center().y);
    cx.simulate_mouse_move(hover, None::<MouseButton>, Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    cx.debug_bounds(TOOLTIP_SELECTOR).is_some()
}

/// `debug_bounds` keys on `'static` strings while the target's selector is built at run
/// time — the test-only leak the `response_viewer` tooltip tests use for the same shape
/// of selector.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

#[test]
fn a_failed_send_names_its_state_instead_of_a_bare_dash() {
    let failed = entry(
        "https://api.test/users",
        None,
        Some(json!({ "error": "timeout" })),
    );
    assert_eq!(
        entry_summary(&failed),
        "Failed",
        "a send that never got a response names its state in the row's meta line"
    );
}

#[test]
fn a_cancelled_send_names_its_state_instead_of_a_bare_dash() {
    let cancelled = entry(
        "https://api.test/users",
        None,
        Some(json!({ "cancelled": true })),
    );
    assert_eq!(
        entry_summary(&cancelled),
        "Cancelled",
        "a cancelled send names its state in the row's meta line"
    );
}

#[gpui::test]
fn a_target_drawn_whole_is_its_own_tooltip(cx: &mut TestAppContext) {
    let store = open_store();
    let id = record(&store, &entry("https://api.test/users", Some(200), None));
    let (_panel, cx) = open_history_panel(store, cx);

    assert!(
        !tooltip_after_hovering_target(id, cx),
        "a URL drawn whole repeats itself in no balloon — the row shows it in full"
    );
}

#[gpui::test]
fn a_truncated_target_carries_its_full_url_in_a_tooltip(cx: &mut TestAppContext) {
    let store = open_store();
    let long = format!("https://api.test/{}", "x".repeat(2000));
    let id = record(&store, &entry(&long, Some(200), None));
    let (_panel, cx) = open_history_panel(store, cx);

    assert!(
        tooltip_after_hovering_target(id, cx),
        "a URL the row truncates shows the whole URL on hover"
    );
}
