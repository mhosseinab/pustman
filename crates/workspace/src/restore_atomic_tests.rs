//! Tab-restore save atomicity and the close flush (issue #85): the stored tab set is
//! replaced all-or-nothing, and closing the window writes what the 2 s debounce has not
//! reached yet instead of losing it. These are the save-path behaviors the reopen
//! round-trips in `restore_tests.rs` cannot pin down — what a save that the store rejects
//! leaves behind, and what a close moments after a change does. The store is the real one
//! (`LocalStore::open_in_memory` + `migrate`) and the clock is advanced, never slept
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{App, Context, Entity, TestAppContext, VisualTestContext, Window};
use storage::LocalStore;
use ui::prelude::*;
use ui::test_support::init_test;

use super::{Snapshot, TabSnapshot, write_snapshot};
use crate::Workspace;
use crate::init;
use crate::item::Item;
use crate::restore::{ItemFactory, Restore};

/// The workspace every test saves under.
const WORKSPACE: &str = "workspace-under-test";

/// A second workspace, to prove a replace stays inside its own.
const OTHER_WORKSPACE: &str = "another-workspace";

/// The local store the persistence runs against: in memory and migrated.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A snapshot of `item_ids` without drafts or layout — the shape `write_snapshot` takes.
fn snapshot(item_ids: &[&str]) -> Snapshot {
    Snapshot {
        layout: None,
        tabs: item_ids
            .iter()
            .map(|item_id| TabSnapshot {
                item_id: (*item_id).to_owned(),
                pinned: false,
                draft: None,
            })
            .collect(),
    }
}

/// The item ids of one workspace's stored tabs, in tab order.
fn stored_item_ids(store: &Arc<Mutex<LocalStore>>, workspace_id: &str) -> Vec<String> {
    let store_guard = store.lock().expect("the lock is free");
    store_guard
        .open_tabs()
        .list(workspace_id)
        .expect("the tabs read back")
        .into_iter()
        .map(|tab| tab.item_id)
        .collect()
}

/// A tab item that knows what it is: its persistent id is its name, and its draft is
/// whatever the test gave it.
struct TestItem {
    id: String,
    draft: Option<String>,
}

impl Render for TestItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Label::new(self.id.clone()))
    }
}

impl Item for TestItem {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::from(self.id.clone())
    }

    fn persistent_id(&self, _cx: &App) -> Option<SharedString> {
        Some(SharedString::from(self.id.clone()))
    }

    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        self.draft.clone()
    }
}

/// A workspace in its own window, on the app's real init order.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// Attach restore to `workspace` over `store`, with a factory that rebuilds [`TestItem`]s
/// — the shape the window-open wiring takes.
fn attach(
    workspace: &Entity<Workspace>,
    store: &Arc<Mutex<LocalStore>>,
    cx: &mut VisualTestContext,
) -> Entity<Restore> {
    let factory: ItemFactory = Arc::new(
        |item_id: &str, draft: Option<String>, _window: &mut Window, cx: &mut App| {
            let item = cx.new(|_| TestItem {
                id: item_id.to_owned(),
                draft,
            });
            Some(Box::new(item))
        },
    );
    cx.update(|window, cx| {
        Restore::attach(
            workspace,
            Arc::clone(store),
            WORKSPACE.to_owned(),
            factory,
            window,
            cx,
        )
    })
}

/// Add one tab per `(id, draft)` to the workspace's pane.
fn add_tabs(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    tabs: &[(&str, Option<&str>)],
) {
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane().clone();
            for &(id, draft) in tabs {
                let item = cx.new(|_| TestItem {
                    id: id.to_owned(),
                    draft: draft.map(str::to_owned),
                });
                pane.update(cx, |pane, cx| pane.add_tab_item(item, cx));
            }
        });
    });
}

/// Issue #85: the tab set is replaced as one step. A set the store rejects — here one
/// holding the same item twice — must not be written halfway: the previously saved tabs
/// stay exactly as they were and the caller is told the save failed.
#[test]
fn a_rejected_tab_set_leaves_the_saved_tabs_untouched() {
    let store = open_store();
    write_snapshot(&store, WORKSPACE, &snapshot(&["one", "two"])).expect("the first save writes");

    let mut rejected = snapshot(&["three", "four"]);
    rejected.tabs.push(TabSnapshot {
        item_id: "four".to_owned(),
        pinned: false,
        draft: None,
    });
    let rejected = write_snapshot(&store, WORKSPACE, &rejected);

    assert!(
        rejected.is_err(),
        "a tab set the store rejects is reported, not half-written"
    );
    assert_eq!(
        stored_item_ids(&store, WORKSPACE),
        ["one", "two"],
        "the rejected save leaves the previously stored tabs untouched"
    );
}

/// The replace touches one workspace's rows and no other's.
#[test]
fn replacing_the_tabs_leaves_other_workspaces_alone() {
    let store = open_store();
    write_snapshot(&store, WORKSPACE, &snapshot(&["one", "two"])).expect("the save writes");
    write_snapshot(&store, OTHER_WORKSPACE, &snapshot(&["other"])).expect("the save writes");

    write_snapshot(&store, WORKSPACE, &snapshot(&["three"])).expect("the save writes");

    assert_eq!(stored_item_ids(&store, WORKSPACE), ["three"]);
    assert_eq!(stored_item_ids(&store, OTHER_WORKSPACE), ["other"]);
}

/// Issue #85: closing the window flushes the save the debounce has not reached. Without
/// the flush, a change made within the debounce window before the close is lost with it.
#[gpui::test]
fn closing_the_window_writes_the_changes_the_debounce_had_not_reached(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let _session = attach(&workspace, &store, cx);
    cx.run_until_parked();

    add_tabs(&workspace, cx, &[("one", Some("draft-one")), ("two", None)]);
    // No settling: the debounced save has not fired when the window closes.
    assert!(
        !cx.simulate_close(),
        "the window closes after the flush, not before it"
    );
    cx.run_until_parked();

    assert_eq!(
        stored_item_ids(&store, WORKSPACE),
        ["one", "two"],
        "closing the window writes the changes the debounce had not reached"
    );
    assert_eq!(
        cx.windows().len(),
        0,
        "the window closes once the flush landed"
    );
}
