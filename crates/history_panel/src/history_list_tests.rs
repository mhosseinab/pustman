//! The history list is driven the way the shell drives it: a real workspace window with
//! the panel registered in its left dock, a migrated in-memory store behind it, and the
//! search through the focused [`TextInput`]. Every test asserts the grouping the view
//! exposes — which entries sit under which day header — so a panel that renders but
//! buckets wrongly or never filters fails them (PRD §8.5, REQ-HIST-01/04).

use std::sync::{Arc, Mutex};

use gpui::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::{HistoryList, HistoryRow};

/// The workspace every test records and reads under.
const WORKSPACE: &str = "workspace-under-test";

/// The two UTC days the fixtures land on: the 18th (newest) and the 17th.
const NEWER_DAY: (i32, u8) = (2026, 18);
const OLDER_DAY: (i32, u8) = (2026, 17);

/// One of the two fixture days as a [`Date`].
fn fixture_day(day: (i32, u8)) -> Date {
    Date::from_calendar_date(day.0, Month::November, day.1).expect("a real calendar date")
}

/// A UTC timestamp on `day` at `hour`:00.
fn timestamp(day: Date, hour: u8) -> OffsetDateTime {
    PrimitiveDateTime::new(
        day,
        Time::from_hms(hour, 0, 0).expect("a whole hour is a valid time"),
    )
    .assume_utc()
}

/// The local store the panel runs against: in memory and migrated
/// (`LocalStore::open_in_memory` + `migrate`, the `workspace` tests' fixture shape).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Record one send and return its stored id.
fn record(
    store: &Arc<Mutex<LocalStore>>,
    method: &str,
    target: &str,
    created_at: OffsetDateTime,
) -> i64 {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
        created_at,
        protocol: "http".to_owned(),
        method: method.to_owned(),
        target: target.to_owned(),
        status: Some(200),
        duration_ms: Some(142),
        request_json: json!({ "method": method, "url": target }),
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

/// Open the panel the way the shell does — in a workspace window, registered in the
/// left dock — and let its first read land.
fn open_history_panel(
    store: Arc<Mutex<LocalStore>>,
    cx: &mut TestAppContext,
) -> (Entity<HistoryList>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the editing keys are unbound and typing into
        // the search box does nothing.
        text_input::init(cx);
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
    (panel, cx)
}

/// The grouping as the view exposes it: one `(day, entry ids)` pair per day header,
/// entries in the order the list draws them.
fn grouping(rows: &[HistoryRow]) -> Vec<(Date, Vec<i64>)> {
    let mut groups: Vec<(Date, Vec<i64>)> = Vec::new();
    for row in rows {
        match row {
            HistoryRow::Day { date, .. } => groups.push((*date, Vec::new())),
            HistoryRow::Entry(entry) => {
                let (_, ids) = groups
                    .last_mut()
                    .expect("an entry always follows its day header");
                ids.push(entry.id.expect("a stored entry carries its id"));
            }
        }
    }
    groups
}

#[gpui::test]
fn entries_grouped_by_day(cx: &mut TestAppContext) {
    let newer_day = fixture_day(NEWER_DAY);
    let older_day = fixture_day(OLDER_DAY);
    let store = open_store();
    let first = record(
        &store,
        "GET",
        "https://api.test/users?page=2",
        timestamp(newer_day, 10),
    );
    let second = record(
        &store,
        "POST",
        "https://api.test/login",
        timestamp(newer_day, 11),
    );
    let third = record(
        &store,
        "GET",
        "https://api.test/status",
        timestamp(older_day, 9),
    );

    let (panel, cx) = open_history_panel(store, cx);

    let groups = cx.read(|cx| panel.read_with(cx, |panel, _| grouping(panel.rows())));
    assert_eq!(
        groups,
        vec![(newer_day, vec![second, first]), (older_day, vec![third])],
        "entries come back newest first, each under its UTC day's header (REQ-HIST-01)"
    );
}

#[gpui::test]
fn search_filters_entries(cx: &mut TestAppContext) {
    let newer_day = fixture_day(NEWER_DAY);
    let older_day = fixture_day(OLDER_DAY);
    let store = open_store();
    let matching_today = record(
        &store,
        "GET",
        "https://api.test/users?page=2",
        timestamp(newer_day, 10),
    );
    let login = record(
        &store,
        "POST",
        "https://api.test/login",
        timestamp(newer_day, 11),
    );
    let matching_earlier = record(
        &store,
        "GET",
        "https://api.test/users/42",
        timestamp(older_day, 9),
    );

    let (panel, cx) = open_history_panel(store, cx);

    let before = cx.read(|cx| panel.read_with(cx, |panel, _| grouping(panel.rows())));
    assert_eq!(
        before,
        vec![
            (newer_day, vec![login, matching_today]),
            (older_day, vec![matching_earlier])
        ],
        "the unfiltered list holds every entry before the search runs"
    );

    // Type into the search box the way a user does: focus the panel, then type.
    let focus_handle = cx.read(|cx| panel.read_with(cx, |panel, cx| panel.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    cx.simulate_input("users");
    cx.run_until_parked();

    let after = cx.read(|cx| panel.read_with(cx, |panel, _| grouping(panel.rows())));
    assert_eq!(
        after,
        vec![
            (newer_day, vec![matching_today]),
            (older_day, vec![matching_earlier])
        ],
        "the search keeps only matching entries, still grouped by day (REQ-HIST-04)"
    );
}
