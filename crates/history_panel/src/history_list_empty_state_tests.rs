//! The History empty states at the design system's empty-state scale (#234): each
//! caption under the search box is an icon, a title and a call to action at readable
//! size — not one tiny muted label. The scenario drives the panel the way the shell
//! does (the `history_list_tests` fixture shape: a real workspace window, the panel in
//! its left dock, a migrated in-memory store), because the fix is about what the panel
//! paints in those states (`docs/postman_redesign_pack/01_specs/design_system.md`,
//! S13/R13).

use std::sync::{Arc, Mutex};

use gpui::{AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext, px};
use serde_json::json;
use storage::{HistoryEntry, LocalStore};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::HistoryList;

/// The workspace every test records and reads under.
const WORKSPACE: &str = "workspace-under-test";

/// The one UTC day the fixture entry lands on.
const FIXTURE_DAY: (i32, u8) = (2026, 18);

/// A UTC timestamp on the fixture day at `hour`:00.
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

/// Record one obviously fake send and return its stored id.
fn record(store: &Arc<Mutex<LocalStore>>, target: &str, created_at: OffsetDateTime) -> i64 {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_owned(),
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

/// Open the panel the way the shell does — in a workspace window, registered in the
/// left dock — and let its first read land.
fn open_history_panel(
    store: Arc<Mutex<LocalStore>>,
    cx: &mut TestAppContext,
) -> (Entity<HistoryList>, &mut VisualTestContext) {
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
    (panel, cx)
}

#[gpui::test]
fn empty_history_shows_an_icon_a_title_and_a_new_request_call_to_action(cx: &mut TestAppContext) {
    let (_panel, cx) = open_history_panel(open_store(), cx);

    assert!(
        cx.debug_bounds("history-empty-state-icon").is_some(),
        "the empty state leads with an icon (design_system.md, Feedback › EmptyState)"
    );
    let title = cx
        .debug_bounds("history-empty-state-title")
        .expect("the empty-state title renders");
    assert!(
        title.size.height >= px(24.),
        "the title reads at the empty-state scale (16 px text and up paints a ≥24 px line \
         box; the old 12 px caption painted 19.5 px), got {title:?}"
    );
    assert!(
        cx.debug_bounds("history-new-request").is_some(),
        "the empty state carries a call to action, not just a caption"
    );
}

#[gpui::test]
fn no_matches_shows_a_clear_search_call_to_action(cx: &mut TestAppContext) {
    let store = open_store();
    record(&store, "https://api.test/users", timestamp(9));
    let (panel, cx) = open_history_panel(store, cx);

    // Type into the search box the way a user does: focus the panel, then type.
    let focus_handle = cx.read(|cx| panel.read_with(cx, |panel, cx| panel.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    cx.simulate_input("zzz");
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("history-empty-state-title").is_some(),
        "a search that matches nothing explains why the list is empty"
    );
    let clear = cx
        .debug_bounds("history-clear-search")
        .expect("the empty state offers a way back to the full list");

    // The call to action works: clicking it drops the query and the rows return.
    cx.simulate_click(clear.center(), Modifiers::none());
    cx.run_until_parked();
    let rows = cx.read(|cx| panel.read_with(cx, |panel, _| panel.rows().len()));
    assert_eq!(
        rows, 2,
        "clearing the search brings the day header and its entry back"
    );
}
