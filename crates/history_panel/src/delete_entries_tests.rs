//! Deleting and clearing history is driven the way the shell drives it: a real
//! workspace window with the panel registered in its left dock and a migrated
//! in-memory store behind it. One scenario walks the three observable promises of
//! PRD REQ-HIST-04 — a delete removes exactly its entry from the store and the list,
//! a cancelled confirm removes nothing, and Confirm empties both — so a wipe that runs
//! without going through the confirm modal fails it.

use std::sync::{Arc, Mutex};

use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use super::{CANCEL_BUTTON_SELECTOR, ClearHistoryModal, HistoryMaintenance};
use crate::history_list::{HistoryList, HistoryRow};

/// The workspace every test records and deletes under.
const WORKSPACE: &str = "workspace-under-test";

/// The one UTC day the fixtures land on.
const FIXTURE_DAY: (i32, u8) = (2026, 18);

/// The fixture day as a [`Date`].
fn fixture_day() -> Date {
    Date::from_calendar_date(FIXTURE_DAY.0, Month::November, FIXTURE_DAY.1)
        .expect("a real calendar date")
}

/// A UTC timestamp on the fixture day at `hour`:00.
fn timestamp(hour: u8) -> OffsetDateTime {
    PrimitiveDateTime::new(
        fixture_day(),
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
) -> (
    Entity<Workspace>,
    Entity<HistoryList>,
    &mut VisualTestContext,
) {
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
    (workspace, panel, cx)
}

/// The stored entry ids, newest first — what the database holds.
fn stored_ids(store: &Arc<Mutex<LocalStore>>) -> Vec<i64> {
    let entries = store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the read succeeds");
    entries
        .into_iter()
        .map(|entry| entry.id.expect("a stored entry carries its id"))
        .collect()
}

/// The entry ids the list draws, in draw order (day headers excluded).
fn shown_ids(panel: &Entity<HistoryList>, cx: &VisualTestContext) -> Vec<i64> {
    cx.read(|cx| {
        panel.read_with(cx, |panel, _| {
            panel
                .rows()
                .iter()
                .filter_map(|row| match row {
                    HistoryRow::Entry(entry) => entry.id,
                    HistoryRow::Day { .. } => None,
                })
                .collect()
        })
    })
}

/// Whether the confirm modal is the workspace's open modal.
fn confirm_modal_is_open(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<ClearHistoryModal>()
                .is_some()
        })
    })
}

#[gpui::test]
fn delete_and_clear_all_remove_entries(cx: &mut TestAppContext) {
    let store = open_store();
    let first = record(
        &store,
        "GET",
        "https://api.test/users?page=2",
        timestamp(10),
    );
    let second = record(&store, "POST", "https://api.test/login", timestamp(11));
    let third = record(&store, "GET", "https://api.test/status", timestamp(12));
    let (workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);
    let maintenance = HistoryMaintenance::new(Arc::clone(&store), WORKSPACE, &panel, &workspace);

    // Deleting one entry by its stable id removes exactly that entry — from the store
    // and from the list — and leaves the others (REQ-HIST-04).
    cx.update(|_, cx| maintenance.delete_entry(second, cx));
    cx.run_until_parked();
    assert_eq!(
        stored_ids(&store),
        vec![third, first],
        "the store keeps only the entries the delete did not name"
    );
    assert_eq!(
        shown_ids(&panel, cx),
        vec![third, first],
        "and the list draws those same entries after its refresh"
    );

    // "Clear all" opens the confirm modal first; a cancelled confirm deletes nothing.
    cx.update(|window, cx| maintenance.clear_all(window, cx));
    draw_window(cx);
    assert!(
        confirm_modal_is_open(&workspace, cx),
        "\"Clear all\" opens the confirm modal before anything is deleted"
    );
    let cancel_bounds = cx
        .debug_bounds(CANCEL_BUTTON_SELECTOR)
        .expect("the Cancel button is on screen while the modal is open");
    cx.simulate_click(cancel_bounds.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        stored_ids(&store),
        vec![third, first],
        "a cancelled confirm deletes nothing"
    );
    assert_eq!(
        shown_ids(&panel, cx),
        vec![third, first],
        "and the list is left as it was"
    );

    // Confirm empties the history — through the keyboard: the modal takes focus when it
    // opens, `focus_next` (Tab) walks its two buttons in reading order, and Enter
    // activates the focused one.
    cx.update(|window, cx| maintenance.clear_all(window, cx));
    draw_window(cx);
    cx.update(|window, cx| window.focus_next(cx));
    cx.update(|window, cx| window.focus_next(cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    assert!(
        stored_ids(&store).is_empty(),
        "Confirm clears every entry from the store"
    );
    assert!(shown_ids(&panel, cx).is_empty(), "and the list is empty");
}
