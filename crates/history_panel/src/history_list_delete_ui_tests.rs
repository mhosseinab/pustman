//! The panel's own delete controls (PRD REQ-HIST-04), driven the way a user drives them:
//! a click on a row's delete button, the delete key binding on the keyboard selection,
//! and the header's "Clear all" through its confirm modal. Every scenario records rows
//! for a second workspace in the same store, so a delete or a wipe that reaches past
//! this panel's workspace fails it.

use std::sync::{Arc, Mutex};

use gpui::{AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::{CLEAR_ALL_ID, HistoryList, HistoryRow, delete_button_selector};
use crate::delete_entries::{CANCEL_BUTTON_SELECTOR, CONFIRM_BUTTON_SELECTOR, ClearHistoryModal};

/// The workspace the panel shows.
const WORKSPACE: &str = "workspace-under-test";

/// Another workspace sharing the same local store: its rows must survive every delete.
const OTHER_WORKSPACE: &str = "other-workspace";

/// The keystroke the delete binding answers on this platform.
const DELETE_KEYSTROKE: &str = if cfg!(target_os = "macos") {
    "cmd-backspace"
} else {
    "shift-delete"
};

/// A UTC timestamp on 2026-11-18 at `hour`:00.
fn timestamp(hour: u8) -> OffsetDateTime {
    let day = Date::from_calendar_date(2026, Month::November, 18).expect("a real calendar date");
    PrimitiveDateTime::new(
        day,
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

/// Record one send under `workspace_id` and return its stored id.
fn record(
    store: &Arc<Mutex<LocalStore>>,
    workspace_id: &str,
    target: &str,
    created_at: OffsetDateTime,
) -> i64 {
    let entry = HistoryEntry {
        id: None,
        workspace_id: workspace_id.to_owned(),
        created_at,
        protocol: "http".to_owned(),
        method: "GET".to_owned(),
        target: target.to_owned(),
        status: Some(200),
        duration_ms: Some(142),
        request_json: json!({ "method": "GET", "url": target }),
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

/// Open the panel the way the shell does — `history_panel::init` run, the panel in a
/// workspace window's left dock — and let its first read land.
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
        crate::init(cx);
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
    draw_window(cx);
    (workspace, panel, cx)
}

/// The stored entry ids of `workspace_id`, newest first.
fn stored_ids(store: &Arc<Mutex<LocalStore>>, workspace_id: &str) -> Vec<i64> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(workspace_id, "")
        .expect("the read succeeds")
        .into_iter()
        .map(|entry| entry.id.expect("a stored entry carries its id"))
        .collect()
}

/// The entry ids the list draws, in draw order.
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

/// Click the element drawn under `selector`. `debug_bounds` takes a `'static` name, so
/// a row's runtime-built selector is leaked — a few bytes per test.
fn click(cx: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_row_delete_button_removes_exactly_that_entry(cx: &mut TestAppContext) {
    let store = open_store();
    let older = record(&store, WORKSPACE, "https://api.test/users", timestamp(10));
    let newer = record(&store, WORKSPACE, "https://api.test/login", timestamp(11));
    let foreign = record(
        &store,
        OTHER_WORKSPACE,
        "https://api.test/other",
        timestamp(12),
    );
    let (workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);

    click(cx, &delete_button_selector(newer));

    assert_eq!(
        stored_ids(&store, WORKSPACE),
        vec![older],
        "the row's delete button removes exactly its entry from the store"
    );
    assert_eq!(
        shown_ids(&panel, cx),
        vec![older],
        "and the list refreshes to draw only the survivor"
    );
    assert_eq!(
        stored_ids(&store, OTHER_WORKSPACE),
        vec![foreign],
        "another workspace's history is untouched"
    );
    let opened = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.center_pane().read(cx).active_item().is_some()
        })
    });
    assert!(!opened, "deleting a row does not also open it");
}

#[gpui::test]
fn test_delete_key_binding_deletes_the_selected_row(cx: &mut TestAppContext) {
    let store = open_store();
    let older = record(&store, WORKSPACE, "https://api.test/users", timestamp(10));
    let newer = record(&store, WORKSPACE, "https://api.test/login", timestamp(11));
    let foreign = record(
        &store,
        OTHER_WORKSPACE,
        "https://api.test/other",
        timestamp(12),
    );
    let (_workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);

    let focus_handle = cx.read(|cx| panel.read_with(cx, |panel, cx| panel.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    // Down twice: onto the newest entry, then the older one.
    cx.simulate_keystrokes("down down");
    cx.run_until_parked();
    cx.simulate_keystrokes(DELETE_KEYSTROKE);
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        stored_ids(&store, WORKSPACE),
        vec![newer],
        "the binding deletes the selected row, not its neighbour"
    );
    assert_eq!(shown_ids(&panel, cx), vec![newer], "and the list follows");
    assert_eq!(stored_ids(&store, OTHER_WORKSPACE), vec![foreign]);
    assert!(
        cx.read(|cx| panel.read_with(cx, |panel, cx| panel.search.read(cx).text().is_empty())),
        "the keystroke never edits the search box"
    );
    assert!(!stored_ids(&store, WORKSPACE).contains(&older));
}

#[gpui::test]
fn test_clear_all_confirms_then_clears_only_this_workspace(cx: &mut TestAppContext) {
    let store = open_store();
    let older = record(&store, WORKSPACE, "https://api.test/users", timestamp(10));
    let newer = record(&store, WORKSPACE, "https://api.test/login", timestamp(11));
    let foreign = record(
        &store,
        OTHER_WORKSPACE,
        "https://api.test/other",
        timestamp(12),
    );
    let (workspace, panel, cx) = open_history_panel(Arc::clone(&store), cx);

    click(cx, CLEAR_ALL_ID);
    assert!(
        confirm_modal_is_open(&workspace, cx),
        "\"Clear all\" asks before it deletes anything"
    );
    click(cx, CANCEL_BUTTON_SELECTOR);
    assert!(!confirm_modal_is_open(&workspace, cx));
    assert_eq!(
        stored_ids(&store, WORKSPACE),
        vec![newer, older],
        "Cancel keeps every entry"
    );
    assert_eq!(shown_ids(&panel, cx), vec![newer, older]);

    click(cx, CLEAR_ALL_ID);
    click(cx, CONFIRM_BUTTON_SELECTOR);
    assert!(
        stored_ids(&store, WORKSPACE).is_empty(),
        "Confirm clears this workspace's history"
    );
    assert!(shown_ids(&panel, cx).is_empty(), "and the list empties");
    assert_eq!(
        stored_ids(&store, OTHER_WORKSPACE),
        vec![foreign],
        "another workspace's history survives Clear all"
    );
}
