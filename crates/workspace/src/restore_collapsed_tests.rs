//! The console strip and the saved layout (S48u5 fix 1): a saved layout wins over the
//! app's resting strip by design — whichever of the two lands first — the `collapsed`
//! flag round-trips through save and restore, and a layout saved before the flag
//! existed still parses (as unfolded, which is what those consoles were).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Entity, TestAppContext, VisualTestContext, Window};
use storage::{Layout, LocalStore};
use ui::test_support::init_test;

use super::{ItemFactory, LayoutDocument, Restore, SAVE_DEBOUNCE};
use crate::{Workspace, init};

const WORKSPACE: &str = "workspace-under-test";

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

fn no_items() -> ItemFactory {
    Arc::new(|_: &str, _: Option<String>, _: &mut Window, _: &mut App| None)
}

fn attach(
    workspace: &Entity<Workspace>,
    store: &Arc<Mutex<LocalStore>>,
    cx: &mut VisualTestContext,
) -> Entity<Restore> {
    cx.update(|window, cx| {
        Restore::attach(
            workspace,
            Arc::clone(store),
            WORKSPACE.to_owned(),
            no_items(),
            window,
            cx,
        )
    })
}

fn settle(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
}

fn rest_as_strip(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.rest_as_strip(cx));
        });
    });
}

fn console_state(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> (bool, bool) {
    cx.read(|cx| {
        let dock = workspace.read(cx).bottom_dock().read(cx);
        (dock.is_open(), dock.is_collapsed())
    })
}

/// Save a layout with the console open and unfolded, the way an earlier session left it.
fn save_expanded_console(store: &Arc<Mutex<LocalStore>>) {
    let json = serde_json::json!({
        "left_dock": { "is_open": true, "size_px": 248.0, "collapsed": false },
        "bottom_dock": { "is_open": true, "size_px": 240.0, "collapsed": false },
    });
    store
        .lock()
        .expect("the store lock is free")
        .layout()
        .insert(&Layout {
            workspace_id: WORKSPACE.to_owned(),
            json,
        })
        .expect("the layout saves");
}

#[gpui::test]
fn test_a_saved_expanded_console_stays_expanded_when_the_strip_default_comes_first(
    cx: &mut TestAppContext,
) {
    cx.update(init_test);
    cx.update(init);
    let store = open_store();
    save_expanded_console(&store);
    let (workspace, cx) = cx.add_window_view(Workspace::new);

    rest_as_strip(&workspace, cx);
    let _restore = attach(&workspace, &store, cx);
    settle(cx);

    assert_eq!(
        console_state(&workspace, cx),
        (true, false),
        "the saved open, unfolded console wins over the resting strip"
    );
}

#[gpui::test]
fn test_a_saved_expanded_console_stays_expanded_when_the_strip_default_comes_last(
    cx: &mut TestAppContext,
) {
    cx.update(init_test);
    cx.update(init);
    let store = open_store();
    save_expanded_console(&store);
    let (workspace, cx) = cx.add_window_view(Workspace::new);

    let _restore = attach(&workspace, &store, cx);
    settle(cx);
    rest_as_strip(&workspace, cx);

    assert_eq!(
        console_state(&workspace, cx),
        (true, false),
        "a resting default that lands after the restore leaves the saved layout alone"
    );
}

#[gpui::test]
fn test_collapsed_round_trips_through_save_and_restore(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(init);
    let store = open_store();
    let (first, cx) = cx.add_window_view(Workspace::new);
    rest_as_strip(&first, cx);
    let _first_session = attach(&first, &store, cx);
    settle(cx);
    // A change after the restore landed, so the save writes the strip as the user has it.
    cx.update(|_, cx| {
        first.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.set_size_px(300., cx));
        });
    });
    settle(cx);
    assert_eq!(console_state(&first, cx), (true, true));

    let (reopened, cx) = cx.add_window_view(Workspace::new);
    assert_eq!(
        console_state(&reopened, cx),
        (true, false),
        "a bare window starts unfolded, so only the restore can fold it"
    );
    let _second_session = attach(&reopened, &store, cx);
    settle(cx);

    assert_eq!(
        console_state(&reopened, cx),
        (true, true),
        "the strip comes back folded from the saved layout"
    );
}

#[gpui::test]
fn test_a_layout_saved_before_collapsed_existed_still_parses_as_unfolded() {
    let old = r#"{"left_dock":{"is_open":true,"size_px":248.0},"bottom_dock":{"is_open":true,"size_px":200.0},"active_item_id":null}"#;
    let layout: LayoutDocument =
        serde_json::from_str(old).expect("an older layout document still parses");
    assert!(
        !layout.bottom_dock.collapsed,
        "an older console reads as unfolded"
    );
    assert!(layout.bottom_dock.is_open);
}
