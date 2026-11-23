//! The failure side of deleting history (PRD REQ-HIST-04): a delete or a "Clear all"
//! whose store refuses the write leaves the history untouched and reports the fixed
//! error toast on the workspace — the path `delete_entries` folds `storage`'s errors
//! into. `LocalStore` takes no faults of its own, so these tests drive a store that
//! refuses every write through the [`HistoryStore`] seam (a local copy of
//! `delete_entries_tests.rs`'s fixtures — existing tests stay read-only).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::{
    CLEAR_FAILURE_MESSAGE, CONFIRM_BUTTON_SELECTOR, DELETE_FAILURE_MESSAGE, HistoryMaintenance,
    HistoryStore,
};
use crate::history_list::HistoryList;

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

/// The local store the panel runs against: in memory and migrated.
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

/// A store whose history writes never reach the database — the failing half of the
/// [`HistoryStore`] seam, which exists so this path can be driven at all.
struct RefusingStore;

impl HistoryStore for RefusingStore {
    fn delete(&self, _workspace_id: &str, _entry_id: i64) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("injected history write failure"))
    }

    fn clear(&self, _workspace_id: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("injected history write failure"))
    }
}

/// Whether `message` is the one toast on screen. The message key must outlive the frame
/// `debug_bounds` reads (it takes `&'static str`), so the selector is leaked — the
/// `toast_layer_tests` shape.
fn toast_shows(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    message: &'static str,
) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(message).leak();
    let one_toast = cx
        .read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
        == 1;
    one_toast && cx.debug_bounds(message_selector).is_some()
}

#[gpui::test]
fn failed_delete_reports_an_error_toast_and_keeps_the_entries(cx: &mut TestAppContext) {
    let store = open_store();
    let first = record(
        &store,
        "GET",
        "https://api.test/users?page=2",
        timestamp(10),
    );
    let second = record(&store, "POST", "https://api.test/login", timestamp(11));
    let (workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);
    let maintenance =
        HistoryMaintenance::over_store(Arc::new(RefusingStore), WORKSPACE, &panel, &workspace);

    cx.update(|_, cx| maintenance.delete_entry(second, cx));
    cx.run_until_parked();

    assert_eq!(
        stored_ids(&store),
        vec![second, first],
        "a delete the store refuses removes nothing"
    );
    assert!(
        toast_shows(&workspace, cx, DELETE_FAILURE_MESSAGE),
        "and the fixed failure message reaches the user as a toast"
    );

    // The failure is an error, not a passing report: it stays well past the auto-dismiss
    // window a report dies at (`ui::Toast`'s own timer) until the user lets it go.
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    assert!(
        toast_shows(&workspace, cx, DELETE_FAILURE_MESSAGE),
        "an error toast does not dismiss itself"
    );
}

#[gpui::test]
fn failed_clear_reports_an_error_toast_and_keeps_the_entries(cx: &mut TestAppContext) {
    let store = open_store();
    let first = record(
        &store,
        "GET",
        "https://api.test/users?page=2",
        timestamp(10),
    );
    let second = record(&store, "POST", "https://api.test/login", timestamp(11));
    let (workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);
    let maintenance =
        HistoryMaintenance::over_store(Arc::new(RefusingStore), WORKSPACE, &panel, &workspace);

    // "Clear all" reaches the store only through the confirm modal's Confirm button.
    cx.update(|window, cx| maintenance.clear_all(window, cx));
    draw_window(cx);
    let confirm_bounds = cx
        .debug_bounds(CONFIRM_BUTTON_SELECTOR)
        .expect("the Confirm button is on screen while the modal is open");
    cx.simulate_click(confirm_bounds.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        stored_ids(&store),
        vec![second, first],
        "a clear the store refuses removes nothing"
    );
    assert!(
        toast_shows(&workspace, cx, CLEAR_FAILURE_MESSAGE),
        "and the fixed failure message reaches the user as a toast"
    );
}
