//! Two open tabs can name the same item — a history entry opened beside the request it
//! recorded. The save still writes, and the restart rebuilds that item once.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Context, Entity, TestAppContext, VisualTestContext, Window};
use storage::LocalStore;
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::item::Item;
use crate::restore::{ItemFactory, Restore, SAVE_DEBOUNCE, WRITE_FAILURE_MESSAGE};
use crate::{Workspace, init};

const WORKSPACE: &str = "workspace-under-test";

struct SameIdItem {
    id: String,
}

impl Render for SameIdItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Label::new(self.id.clone()))
    }
}

impl Item for SameIdItem {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::from(self.id.clone())
    }

    fn persistent_id(&self, _cx: &App) -> Option<SharedString> {
        Some(SharedString::from(self.id.clone()))
    }

    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        None
    }
}

fn factory() -> ItemFactory {
    Arc::new(
        |item_id: &str, _draft: Option<String>, _window: &mut Window, cx: &mut App| {
            let item = cx.new(|_| SameIdItem {
                id: item_id.to_owned(),
            });
            Some(Box::new(item) as Box<dyn crate::item::ItemHandle>)
        },
    )
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
            factory(),
            window,
            cx,
        )
    })
}

fn open_ids(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .tabs
                .iter()
                .filter_map(|tab| tab.handle.persistent_id(cx).map(|id| id.to_string()))
                .collect()
        })
    })
}

#[gpui::test]
fn test_two_tabs_with_one_item_id_still_save_and_restore_once(cx: &mut TestAppContext) {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let _first_session = attach(&workspace, &store, cx);

    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane().clone();
            for id in ["request", "request", "other"] {
                let item = cx.new(|_| SameIdItem { id: id.to_owned() });
                pane.update(cx, |pane, cx| pane.add_tab_item(item, cx));
            }
        });
    });
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
    draw_window(cx);

    let failure: &'static str = toast_message_selector(WRITE_FAILURE_MESSAGE).leak();
    assert!(
        cx.debug_bounds(failure).is_none(),
        "a repeated item id does not fail the save"
    );

    let (reopened, cx) = cx.add_window_view(Workspace::new);
    let _second_session = attach(&reopened, &store, cx);
    cx.run_until_parked();
    assert_eq!(
        open_ids(&reopened, cx),
        ["request", "other"],
        "the repeated item comes back once, where it first stood"
    );
}
