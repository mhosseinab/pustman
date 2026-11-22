//! Redundant autosave writes (issue #97): a draft that stays dirty but unchanged must
//! not be rewritten to storage on every tick. Whether a write *happened* is observed
//! through the user-visible consequence of one — a failed save reports itself — so once
//! the first save has landed the store's schema is rolled back
//! (`LocalStore::migrate_down_to(0)` leaves no local-state table behind): every later
//! write would fail, and its failure would reach the user. The store is the real one
//! (`LocalStore::open_in_memory` + `migrate`) and the clock is advanced, never slept
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Context, Entity, TestAppContext, VisualTestContext, Window};
use storage::LocalStore;
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::item::Item;
use crate::restore::{ItemFactory, Restore, SAVE_DEBOUNCE};
use crate::{Autosave, Workspace, init};

/// The workspace every test autosaves under.
const WORKSPACE: &str = "workspace-under-test";

/// The one message a failed save shows — `restore`'s fixed text, asserted as the user
/// sees it.
const WRITE_FAILURE_MESSAGE: &str = "The tabs and layout could not be saved";

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

/// Make every further save fail: the schema is rolled back to nothing, so a write that
/// happens now cannot land — and, because a failed save is reported, is visible. The
/// rows saved before go with the schema; these tests only need the write path's verdict.
fn break_further_writes(store: &Arc<Mutex<LocalStore>>) {
    store
        .lock()
        .expect("the lock is free")
        .migrate_down_to(0)
        .expect("the schema rolls back");
}

/// Whether the user has been told that a save failed.
fn save_failed(cx: &mut VisualTestContext) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(WRITE_FAILURE_MESSAGE).leak();
    cx.debug_bounds(message_selector).is_some()
}

/// Issue #97: a draft that stays dirty but unchanged is not rewritten to storage on
/// every tick. The first save has landed when the store is broken; ticking past the
/// cadence with nothing typed must leave the user with nothing to be told.
#[gpui::test]
fn an_unchanged_dirty_draft_is_not_rewritten_every_tick(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let restore = attach(&workspace, &store, cx);
    let _autosave = cx.update(|_, cx| Autosave::start(&restore, &workspace, cx));

    let items = add_tabs(&workspace, cx, &[("one", Some("draft"))]);
    settle(cx);
    // The draft is dirty now — and stays exactly as it is from here on.
    cx.update(|_, cx| {
        items[0].update(cx, |item, _cx| item.dirty = true);
    });
    break_further_writes(&store);

    for tick in 1..=2 {
        cx.executor()
            .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
        cx.run_until_parked();
        assert!(
            !save_failed(cx),
            "tick {tick} rewrote an unchanged dirty draft (issue #97)"
        );
    }
}

/// The counterpart to [`an_unchanged_dirty_draft_is_not_rewritten_every_tick`]: a draft
/// that *did* change still reaches the write path — and its failure is told to the user
/// here, because the store refuses it. Without this, the test above could pass by never
/// writing anything at all.
#[gpui::test]
fn a_changed_draft_still_reaches_the_write_path(cx: &mut TestAppContext) {
    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    let restore = attach(&workspace, &store, cx);
    let _autosave = cx.update(|_, cx| Autosave::start(&restore, &workspace, cx));

    let items = add_tabs(&workspace, cx, &[("one", Some("draft"))]);
    settle(cx);
    break_further_writes(&store);
    cx.update(|_, cx| {
        items[0].update(cx, |item, _cx| {
            item.draft = Some("draft-new".to_owned());
            item.dirty = true;
        });
    });

    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
    assert!(
        save_failed(cx),
        "a changed draft is written — and a store that refuses it reaches the user"
    );
}
