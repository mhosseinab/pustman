//! The title-bar Close control and the save-then-close flush (issue #85): a
//! client-decorated window's own X goes through the shell, and it must run the same
//! flush-then-close path as a platform close — a change made within the 2 s debounce
//! window must not be lost to the app's own close button. The store is the real one
//! (`LocalStore::open_in_memory` + `migrate`) and the clock is advanced, never slept
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{
    App, Context, Decorations, Entity, Modifiers, TestAppContext, Tiling, VisualTestContext, Window,
};
use storage::LocalStore;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::CLOSE_SELECTOR;
use crate::Workspace;
use crate::init;
use crate::item::Item;
use crate::restore::{ItemFactory, Restore};

/// The workspace every test saves under.
const WORKSPACE: &str = "workspace-under-test";

/// The local store the persistence runs against: in memory and migrated.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
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

/// Issue #85: clicking the title-bar Close flushes the save the debounce has not reached
/// before the window goes — the same path a platform close takes.
#[gpui::test]
fn clicking_the_title_bar_close_flushes_the_pending_save(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let _session = attach(&workspace, &store, cx);
    cx.run_until_parked();

    // The title bar is the app's own only in a client-decorated window.
    let window = cx.windows()[0];
    cx.simulate_window_decorations_change(
        window,
        Decorations::Client {
            tiling: Tiling::default(),
        },
    );
    draw_window(cx);

    add_tabs(&workspace, cx, &[("one", Some("draft-one")), ("two", None)]);
    // No settling: the debounced save has not fired when the close button is clicked.
    let close = cx
        .debug_bounds(CLOSE_SELECTOR)
        .expect("the title bar shows a close button");
    cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        stored_item_ids(&store, WORKSPACE),
        ["one", "two"],
        "clicking the title-bar close writes the changes the debounce had not reached"
    );
    assert!(
        cx.windows().is_empty(),
        "the window closes once the flush landed"
    );
}
