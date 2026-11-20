//! Draft autosave (PRD REQ-NFR-05): a dirty tab's typed draft reaches `storage` within
//! the 2 s cadence even though typing into an item notifies nobody — not the item's
//! observers, not the pane, not [`crate::Restore`]'s. The store is the real one
//! (`LocalStore::open_in_memory` + `migrate`, as the driver asked) and the clock is
//! advanced, never slept (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Context, Entity, TestAppContext, VisualTestContext, Window};
use storage::LocalStore;
use ui::prelude::*;
use ui::test_support::init_test;

use crate::item::Item;
use crate::restore::{ItemFactory, Restore, SAVE_DEBOUNCE};
use crate::{Autosave, Workspace, init};

/// The workspace every test autosaves under.
const WORKSPACE: &str = "workspace-under-test";

/// An item that knows what it is: its persistent id is its name, its draft is whatever
/// the test gave it, and the test drives `dirty` by hand — the smallest stand-in for a
/// request editor tab (`restore_tests.rs`'s fixture, plus the dirty flag it lacks).
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

/// The local store the persistence runs against: in memory and migrated
/// (`LocalStore::open_in_memory` + `migrate`, as the driver asked).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace in its own window, on the app's real init order.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// The factory that rebuilds [`TestItem`]s from their stored ids and drafts.
fn test_factory() -> ItemFactory {
    Arc::new(
        |item_id: &str, draft: Option<String>, _window: &mut Window, cx: &mut App| {
            let item = cx.new(|_| TestItem {
                id: item_id.to_owned(),
                draft,
                dirty: false,
            });
            Some(Box::new(item))
        },
    )
}

/// Attach restore to `workspace` over `store` — the shape the window-open wiring takes.
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
            test_factory(),
            window,
            cx,
        )
    })
}

/// Add one tab per `(id, draft)` to the workspace's pane and return the items, so a test
/// can type into them the way a user would.
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
                    dirty: false,
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

/// The `open_tabs` rows as the tests see them: `(item id, draft)` per tab, in order.
fn stored_drafts(store: &Arc<Mutex<LocalStore>>) -> Vec<(String, Option<String>)> {
    let store = store.lock().expect("the lock is free");
    store
        .open_tabs()
        .list(WORKSPACE)
        .expect("the tabs list")
        .into_iter()
        .map(|row| {
            (
                row.item_id,
                row.draft_json.map(|value| match value {
                    serde_json::Value::String(text) => text,
                    other => other.to_string(),
                }),
            )
        })
        .collect()
}

#[gpui::test]
fn draft_autosaved_after_two_seconds(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let restore = attach(&workspace, &store, cx);
    let _autosave = cx.update(|_, cx| Autosave::start(&restore, &workspace, cx));

    let items = add_tabs(&workspace, cx, &[("one", Some("draft-old"))]);
    settle(cx);
    assert_eq!(
        stored_drafts(&store),
        vec![("one".to_owned(), Some("draft-old".to_owned()))],
        "the tab's first draft is stored before the typing starts"
    );

    // The user types: the draft changes on the item alone, with no `notify` — so neither
    // the pane's item observer nor the restore's pane observer hears about it.
    cx.update(|_, cx| {
        items[0].update(cx, |item, _cx| {
            item.draft = Some("draft-new".to_owned());
            item.dirty = true;
        });
    });

    // Just under the cadence since the autosave's last tick: still the old draft.
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE - Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(
        stored_drafts(&store),
        vec![("one".to_owned(), Some("draft-old".to_owned()))],
        "before the tick comes, the store keeps the old draft"
    );

    // Past the cadence: the tick wrote the draft through the restore path.
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(
        stored_drafts(&store),
        vec![("one".to_owned(), Some("draft-new".to_owned()))],
        "the dirty draft is stored within the 2 s cadence (REQ-NFR-05)"
    );
}
