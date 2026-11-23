//! The day headers follow the day (#110): "Today" and "Yesterday" are derived against
//! the UTC day the list is drawn on, never frozen by the read that grouped the rows —
//! so a panel left open across UTC midnight re-derives them instead of keeping
//! yesterday's "Today".

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};

use super::{HistoryList, HistoryRow, day_label, until_next_utc_midnight};

/// The workspace every recorded row belongs to.
const WORKSPACE: &str = "workspace-under-test";

/// The day the fixture entries land on.
fn fixture_day() -> Date {
    Date::from_calendar_date(2026, Month::November, 18).expect("a real calendar date")
}

/// A UTC timestamp on `day` at noon.
fn timestamp(day: Date) -> OffsetDateTime {
    PrimitiveDateTime::new(
        day,
        Time::from_hms(12, 0, 0).expect("a whole hour is a valid time"),
    )
    .assume_utc()
}

/// The in-memory store the panel reads (the `history_list_tests` fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one fixture send on `day` and return its stored id.
fn record(store: &Arc<Mutex<LocalStore>>, day: Date) -> i64 {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at: timestamp(day),
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

/// The day header labels the list draws, top to bottom.
fn day_labels(panel: &Entity<HistoryList>, cx: &VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, _| {
        panel
            .rows()
            .iter()
            .filter_map(|row| match row {
                HistoryRow::Day { label, .. } => Some(label.to_string()),
                HistoryRow::Entry(_) => None,
            })
            .collect()
    })
}

#[test]
fn day_labels_name_today_yesterday_and_the_iso_date() {
    let today = fixture_day();
    let yesterday = today
        .previous_day()
        .expect("the day before the fixture day");
    let older = yesterday
        .previous_day()
        .expect("two days before the fixture day");
    assert_eq!(day_label(today, today), "Today");
    assert_eq!(day_label(yesterday, today), "Yesterday");
    assert_eq!(day_label(older, today), "2026-11-16");
}

#[test]
fn the_day_label_wake_waits_for_the_next_utc_midnight() {
    let day = fixture_day();
    let at = |hour: u8, minute: u8, second: u8| {
        PrimitiveDateTime::new(
            day,
            Time::from_hms(hour, minute, second).expect("a whole second is a valid time"),
        )
        .assume_utc()
    };
    assert_eq!(
        until_next_utc_midnight(at(12, 0, 0)),
        Duration::from_secs(43_200)
    );
    assert_eq!(
        until_next_utc_midnight(at(23, 59, 30)),
        Duration::from_secs(30)
    );
    assert_eq!(
        until_next_utc_midnight(at(0, 0, 1)),
        Duration::from_secs(86_399),
        "a wake fired just after midnight waits for the next one"
    );
}

#[gpui::test]
fn a_panel_left_open_across_utc_midnight_re_derives_its_day_labels(cx: &mut TestAppContext) {
    let day = fixture_day();
    let store = open_store();
    record(&store, day);
    let (panel, cx) = open_history_panel(store, cx);

    panel.update(cx, |panel, cx| panel.refresh_day_labels(day, cx));
    assert_eq!(
        day_labels(&panel, cx),
        vec![String::from("Today")],
        "the day the panel is drawn on is 'Today'"
    );

    // The panel is left open across a UTC midnight: the wake re-derives the labels.
    let next_day = day.next_day().expect("the day after the fixture day");
    panel.update(cx, |panel, cx| panel.refresh_day_labels(next_day, cx));
    assert_eq!(
        day_labels(&panel, cx),
        vec![String::from("Yesterday")],
        "a list left open across UTC midnight does not keep yesterday's 'Today'"
    );
}
