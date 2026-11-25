//! The status bar's controls and the collapsed console (S48u5), on the app's real init
//! order: a first launch (no saved layout) starts the console as its one-line strip,
//! and the status bar's Console, Cookies and Sidebar controls each run their action when
//! clicked — nothing in the bar does nothing.

use std::sync::Arc;
use std::time::Duration;

use console::{ConsoleEntry, ConsoleLevel, ConsoleStore};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use response_viewer::cookie_dialog::CookieDialog;
use storage::Layout;
use ui::test_support::draw_window;
use workspace::Workspace;

use crate::status_items::{CONSOLE_TOGGLE_SELECTOR, COOKIES_SELECTOR, SIDEBAR_TOGGLE_SELECTOR};
use crate::{AppStore, StoreLocation, init_with_store};

fn first_launch(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| crate::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

fn console_state(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> (bool, bool) {
    cx.read(|cx| {
        let dock = workspace.read(cx).bottom_dock().read(cx);
        (dock.is_open(), dock.is_collapsed())
    })
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_a_first_launch_starts_the_console_collapsed_to_its_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = first_launch(cx);
    assert_eq!(
        console_state(&workspace, cx),
        (true, true),
        "with no saved layout the console starts as its strip"
    );
    let dock = cx.debug_bounds("bottom-dock").expect("the strip renders");
    assert!(
        dock.size.height < gpui::px(64.),
        "the strip is one line tall, not the 200 px pane: {dock:?}"
    );
}

#[gpui::test]
fn test_the_status_bar_console_control_moves_between_strip_and_full_console(
    cx: &mut TestAppContext,
) {
    let (workspace, cx) = first_launch(cx);

    click(cx, CONSOLE_TOGGLE_SELECTOR);
    assert_eq!(
        console_state(&workspace, cx),
        (true, false),
        "the Console control unfolds the strip"
    );

    click(cx, CONSOLE_TOGGLE_SELECTOR);
    assert_eq!(
        console_state(&workspace, cx),
        (true, true),
        "clicking it again folds the console back to its strip, never hides it"
    );
}

#[gpui::test]
fn test_new_console_entries_leave_the_strip_folded(cx: &mut TestAppContext) {
    let (workspace, cx) = first_launch(cx);
    cx.update(|_, cx| {
        ConsoleStore::push(
            cx,
            ConsoleEntry {
                level: ConsoleLevel::Error,
                protocol: "http".to_owned(),
                method: None,
                url: "http://example.test/failed".to_owned(),
                status: Some(500),
                time: Some(Duration::from_millis(7)),
                size: Some(0),
                raw_request: String::new(),
                raw_response: String::new(),
            },
        );
    });
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        console_state(&workspace, cx),
        (true, true),
        "an entry, even an error, does not unfold the console by itself"
    );
}

#[gpui::test]
fn test_a_window_with_a_saved_expanded_console_opens_expanded(cx: &mut TestAppContext) {
    let (_first, cx) = first_launch(cx);
    cx.update(|_, cx| {
        let store = Arc::clone(&cx.global::<AppStore>().0);
        let json = serde_json::json!({
            "left_dock": { "is_open": true, "size_px": 248.0, "collapsed": false },
            "bottom_dock": { "is_open": true, "size_px": 240.0, "collapsed": false },
        });
        store
            .lock()
            .expect("the store lock is free")
            .layout()
            .insert(&Layout {
                workspace_id: crate::WORKSPACE_ID.to_owned(),
                json,
            })
            .expect("the layout saves");
    });

    let (reopened, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        console_state(&reopened, cx),
        (true, false),
        "the saved open console wins over the app's resting strip"
    );
}

#[gpui::test]
fn test_the_status_bar_sidebar_control_toggles_the_sidebar(cx: &mut TestAppContext) {
    let (workspace, cx) = first_launch(cx);
    let left_open =
        |cx: &VisualTestContext| cx.read(|cx| workspace.read(cx).left_dock().read(cx).is_open());
    assert!(left_open(cx), "the sidebar starts open");

    click(cx, SIDEBAR_TOGGLE_SELECTOR);
    assert!(!left_open(cx), "the Sidebar control hides the sidebar");

    click(cx, SIDEBAR_TOGGLE_SELECTOR);
    assert!(left_open(cx), "and shows it again");
}

#[gpui::test]
fn test_the_status_bar_cookies_control_opens_manage_cookies(cx: &mut TestAppContext) {
    let (workspace, cx) = first_launch(cx);

    click(cx, COOKIES_SELECTOR);

    assert!(
        cx.read(|cx| {
            workspace
                .read(cx)
                .modal_layer()
                .active_modal::<CookieDialog>()
                .is_some()
        }),
        "the Cookies control runs cookies::Manage, which opens the dialog"
    );
}
