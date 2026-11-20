//! The restore and autosave wire (S23w): what one launch had open comes back in the next
//! one — tabs with their drafts, the active tab and the dock sizes — and a draft typed
//! after the last change reaches the store on the autosave tick alone. One store is
//! shared by both simulated launches, injected as `StoreLocation::Shared`: a fresh
//! in-memory store per launch would prove nothing, because the second launch would read
//! an empty database. The clock is advanced, never slept (`.agents/rules/tests.md`).

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Context, Entity, EntityId, TestAppContext, VisualTestContext, Window};
use pustman::{StoreLocation, restore_wiring};
use storage::LocalStore;
use ui::prelude::*;
use workspace::{Item, ItemFactory, Workspace};

/// The save cadence `workspace` debounces and autosaves on — its `SAVE_DEBOUNCE`, which
/// the crate does not re-export.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// An item that knows what it is: its persistent id is its name, its draft is whatever
/// the test gave it, and the test drives `dirty` by hand — the smallest stand-in for a
/// request editor tab.
struct TestItem {
    id: String,
    draft: Option<String>,
    dirty: bool,
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

    fn is_dirty(&self, _cx: &App) -> bool {
        self.dirty
    }

    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        self.draft.clone()
    }

    fn persistent_id(&self, _cx: &App) -> Option<SharedString> {
        Some(SharedString::from(self.id.clone()))
    }
}

/// What the test's item kind records: every `(item id, draft)` pair it rebuilt and the
/// entity it built, so a test can read the pane's tab order by entity id.
type RebuiltTabs = Arc<Mutex<Vec<(String, Option<String>, EntityId)>>>;

/// The one store both launches run over: in memory and migrated, shared by injection.
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The item kind the app rebuilds tabs with here — the shape a feature crate registers
/// (S26w adds the request editor kind).
fn test_item_kind() -> (ItemFactory, RebuiltTabs) {
    let rebuilt = Arc::new(Mutex::new(Vec::new()));
    let kind: ItemFactory = Arc::new({
        let rebuilt = Arc::clone(&rebuilt);
        move |item_id: &str, draft: Option<String>, _window: &mut Window, cx: &mut App| {
            let item = cx.new(|_| TestItem {
                id: item_id.to_owned(),
                draft: draft.clone(),
                dirty: false,
            });
            rebuilt.lock().expect("the lock is free").push((
                item_id.to_owned(),
                draft,
                item.entity_id(),
            ));
            Some(Box::new(item))
        }
    });
    (kind, rebuilt)
}

/// Add one tab per id to the workspace's pane — the tabs the first launch left open.
fn add_tabs(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    ids: &[&str],
) -> Vec<Entity<TestItem>> {
    cx.update(|_, cx| {
        let mut items = Vec::new();
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane().clone();
            for id in ids {
                let item = cx.new(|_| TestItem {
                    id: (*id).to_owned(),
                    draft: None,
                    dirty: false,
                });
                pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
                items.push(item);
            }
        });
        items
    })
}

/// Let the debounced save — and one autosave tick — write through.
fn settle(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
}

#[gpui::test]
fn wire_restore_reopens_tabs_and_layout_after_relaunch(cx: &mut TestAppContext) {
    let store = shared_store();
    let (kind, rebuilt) = test_item_kind();
    cx.update(|cx| restore_wiring::register_item_kind(cx, kind));

    // Launch 1: the app's init order over the shared store, then its one window.
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();

    let items = add_tabs(&workspace, cx, &["one", "two"]);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.set_size_px(320., cx));
            let pane = workspace.center_pane().clone();
            pane.update(cx, |pane, cx| pane.set_active_index(1, cx));
        });
    });
    settle(cx);

    // A draft typed after that: no observer hears it, so only the autosave tick can
    // carry it to the store (REQ-NFR-05).
    cx.update(|_, cx| {
        items[0].update(cx, |item, _cx| {
            item.draft = Some("typed-later".to_owned());
            item.dirty = true;
        });
    });
    settle(cx);

    // Launch 2: a fresh init over the same store; its window must come back whole.
    cx.update(|_window, cx| {
        pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store)))
    });
    let (relaunched, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();

    let rebuilt = rebuilt.lock().expect("the lock is free");
    assert_eq!(
        rebuilt
            .iter()
            .map(|(item_id, draft, _)| (item_id.clone(), draft.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("one".to_owned(), Some("typed-later".to_owned())),
            ("two".to_owned(), None),
        ],
        "the tabs are rebuilt with the drafts the autosave saved (REQ-NFR-05)"
    );
    let (order, active, bottom_size) = cx.read(|cx| {
        relaunched.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.item_ids(),
                pane.active_item()
                    .and_then(|item| item.persistent_id(cx))
                    .map(|item_id| item_id.to_string()),
                workspace.bottom_dock().read(cx).size_px(),
            )
        })
    });
    assert_eq!(
        order,
        vec![rebuilt[0].2, rebuilt[1].2],
        "the tabs come back in the order they were open in (REQ-UI-02)"
    );
    assert_eq!(
        active.as_deref(),
        Some("two"),
        "the active tab comes back active (REQ-UI-02)"
    );
    assert_eq!(
        bottom_size, 320.,
        "the dock size comes back after the relaunch (REQ-UI-01)"
    );
}
