//! Layout and tab restore (PRD REQ-UI-01/02, REQ-NFR-05): dock sizes and visibility
//! survive a reopen, tabs come back in order with their pins, active tab and draft, and a
//! tab whose item is gone is reported in a toast instead of restored. The store is the
//! real one (`LocalStore::open_in_memory` + `migrate`, as the driver asked) — only the
//! items are test items, rebuilt by the caller's factory the way a feature crate's would
//! be. The clock is advanced, never slept (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Context, Entity, TestAppContext, VisualTestContext, Window};
use storage::LocalStore;
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::item::Item;
use crate::restore::{ItemFactory, Restore, SAVE_DEBOUNCE};
use crate::{Workspace, init};

/// The workspace every test saves and restores under.
const WORKSPACE: &str = "workspace-under-test";

/// What a test's factory records: every `(item id, draft)` pair it was asked to rebuild.
type FactoryRequests = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// An item that knows what it is: its persistent id is its name, and its draft is
/// whatever the test gave it — the smallest stand-in for a request editor tab.
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

/// The local store the persistence runs against: in memory and migrated
/// (`LocalStore::open_in_memory` + `migrate`, as the driver asked).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace in its own window, on the app's real init order — the first session.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// A second window over the same app — the restart. The app's init order ran once.
fn reopened_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.add_window_view(Workspace::new)
}

/// The factory that rebuilds [`TestItem`]s from their stored ids and drafts, recording
/// every `(id, draft)` pair it was asked for.
fn test_factory() -> (ItemFactory, FactoryRequests) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let factory: ItemFactory = Arc::new({
        let requests = Arc::clone(&requests);
        move |item_id: &str, draft: Option<String>, _window: &mut Window, cx: &mut App| {
            requests
                .lock()
                .expect("the lock is free")
                .push((item_id.to_owned(), draft.clone()));
            let item = cx.new(|_| TestItem {
                id: item_id.to_owned(),
                draft,
            });
            Some(Box::new(item))
        }
    });
    (factory, requests)
}

/// Attach restore to `workspace` over `store` — the shape the window-open wiring takes.
fn attach(
    workspace: &Entity<Workspace>,
    store: &Arc<Mutex<LocalStore>>,
    factory: ItemFactory,
    cx: &mut VisualTestContext,
) -> Entity<Restore> {
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

/// Add one tab per `(id, draft)` to the workspace's pane and return the items, so a test
/// can drive the pane by entity id the way a user's clicks would.
fn add_tabs(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    tabs: &[(&str, Option<&str>)],
) -> Vec<Entity<TestItem>> {
    cx.update(|_, cx| {
        let mut items = Vec::new();
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane().clone();
            for &(id, draft) in tabs {
                let item = cx.new(|_| TestItem {
                    id: id.to_owned(),
                    draft: draft.map(str::to_owned),
                });
                pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
                items.push(item);
            }
        });
        items
    })
}

/// Let the debounced save write through: advance past the debounce and drain the tasks.
fn settle(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
}

/// The pane's tabs as the tests see them: persistent ids in tab order, the pinned flags
/// beside them, and the active tab's persistent id.
fn tab_state(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> (Vec<String>, Vec<bool>, Option<String>) {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.tabs
                    .iter()
                    .map(|tab| {
                        tab.handle
                            .persistent_id(cx)
                            .map(|id| id.to_string())
                            .unwrap_or_default()
                    })
                    .collect(),
                pane.tabs.iter().map(|tab| tab.pinned).collect(),
                pane.active_item()
                    .and_then(|item| item.persistent_id(cx))
                    .map(|id| id.to_string()),
            )
        })
    })
}

#[gpui::test]
fn layout_restored_after_reopen(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let (factory, _requests) = test_factory();
    let _first_session = attach(&workspace, &store, factory, cx);

    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .left_dock()
                .update(cx, |dock, cx| dock.set_open(false, cx));
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.set_size_px(320., cx));
        });
    });
    settle(cx);
    draw_window(cx);
    let saved_bottom_height = cx
        .debug_bounds("bottom-dock")
        .expect("the resized bottom dock renders")
        .size
        .height;

    let (reopened, cx) = reopened_window(cx);
    let (factory, _requests) = test_factory();
    let _second_session = attach(&reopened, &store, factory, cx);
    cx.run_until_parked();
    draw_window(cx);

    let (left_open, bottom_size) = cx.read(|cx| {
        reopened.read_with(cx, |workspace, cx| {
            (
                workspace.left_dock().read(cx).is_open(),
                workspace.bottom_dock().read(cx).size_px(),
            )
        })
    });
    assert!(
        !left_open,
        "the left dock that was closed stays closed after a restart (REQ-UI-01)"
    );
    assert!(
        cx.debug_bounds("left-dock").is_none(),
        "the restored closed left dock renders nothing"
    );
    assert_eq!(
        bottom_size, 320.,
        "the bottom dock keeps its saved size (REQ-UI-01/04)"
    );
    let restored_bottom_height = cx
        .debug_bounds("bottom-dock")
        .expect("the restored bottom dock renders")
        .size
        .height;
    assert_eq!(
        restored_bottom_height, saved_bottom_height,
        "the restored bottom dock renders at the height it was saved at"
    );
}

#[gpui::test]
fn tabs_restored_in_order_with_active_and_pinned(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let (factory, _requests) = test_factory();
    let _first_session = attach(&workspace, &store, factory, cx);

    let items = add_tabs(
        &workspace,
        cx,
        &[("one", None), ("two", Some("draft-two")), ("three", None)],
    );
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane().clone();
            pane.update(cx, |pane, cx| {
                pane.set_pinned(items[0].entity_id(), true, cx);
                pane.set_active_index(1, cx);
            });
        });
    });
    settle(cx);

    let (reopened, cx) = reopened_window(cx);
    let (factory, requests) = test_factory();
    let _second_session = attach(&reopened, &store, factory, cx);
    cx.run_until_parked();

    let (order, pinned, active) = tab_state(&reopened, cx);
    assert_eq!(
        order,
        ["one", "two", "three"],
        "the tabs come back in the order they were open in"
    );
    assert_eq!(
        pinned,
        [true, false, false],
        "the pinned tab comes back pinned"
    );
    assert_eq!(
        active.as_deref(),
        Some("two"),
        "the active tab comes back active"
    );
    assert_eq!(
        requests.lock().expect("the lock is free").clone(),
        vec![
            ("one".to_owned(), None),
            ("two".to_owned(), Some("draft-two".to_owned())),
            ("three".to_owned(), None),
        ],
        "each tab is rebuilt with the draft its item serialized (REQ-NFR-05)"
    );
}

#[gpui::test]
fn missing_item_reported_not_restored(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let (factory, _requests) = test_factory();
    let _first_session = attach(&workspace, &store, factory, cx);

    add_tabs(&workspace, cx, &[("one", None), ("ghost", None)]);
    settle(cx);

    let (reopened, cx) = reopened_window(cx);
    let (factory, requests) = test_factory();
    let factory: ItemFactory = Arc::new(move |item_id: &str, draft, window, cx| {
        if item_id == "ghost" {
            return None;
        }
        factory(item_id, draft, window, cx)
    });
    let _second_session = attach(&reopened, &store, factory, cx);
    cx.run_until_parked();

    let (order, _pinned, active) = tab_state(&reopened, cx);
    assert_eq!(
        order,
        ["one"],
        "the tab whose item is gone is not restored (REQ-UI-02)"
    );
    assert_eq!(
        active.as_deref(),
        Some("one"),
        "the pane falls back to a tab that did come back"
    );
    assert_eq!(
        requests.lock().expect("the lock is free").clone(),
        vec![("one".to_owned(), None)],
        "the factory is only asked for items that can still exist"
    );

    draw_window(cx);
    let message_selector: &'static str =
        toast_message_selector("Couldn't restore tabs; missing items: ghost").leak();
    assert!(
        cx.debug_bounds(message_selector).is_some(),
        "the vanished item is reported to the user in a toast"
    );
}
