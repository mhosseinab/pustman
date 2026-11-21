//! One tab per item on the pane strip (REQ-UI-02): adding an item that is already open
//! activates its existing tab instead of pushing a second one. A duplicate tab would
//! share its element id with the first one in the same frame, and `close_tab` would
//! close only one of the pair.

use gpui::{
    App, Context, Entity, EntityId, IntoElement, ParentElement, Render, SharedString,
    TestAppContext, VisualTestContext, Window,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::item::Item;
use crate::{Workspace, init};

/// An item the test adds to the pane, possibly more than once.
struct TestItem {
    title: &'static str,
}

impl Render for TestItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Label::new(self.title))
    }
}

impl Item for TestItem {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::from(self.title)
    }
}

/// A workspace in its own window, on the app's real init order.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// The open items and the active one, as the center pane sees them.
fn pane_state(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> (Vec<EntityId>, Option<EntityId>) {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.item_ids(),
                pane.active_item().map(|item| item.entity_id()),
            )
        })
    })
}

#[gpui::test]
fn adding_an_item_twice_keeps_one_tab_and_activates_it(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let item = cx.update(|_, cx| cx.new(|_| TestItem { title: "request" }));
    let item_id = item.entity_id();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane();
            pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
            pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
        });
    });
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(
        items,
        vec![item_id],
        "adding the same item twice leaves one tab, not two sharing one element id"
    );
    assert_eq!(
        active,
        Some(item_id),
        "the second add activates the tab that is already open"
    );
}

#[gpui::test]
fn closing_the_single_tab_of_a_twice_added_item_empties_the_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let item = cx.update(|_, cx| cx.new(|_| TestItem { title: "request" }));
    let item_id = item.entity_id();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let pane = workspace.center_pane();
            pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
            pane.update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
        });
    });

    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.close_tab(item_id, cx));
        });
    });
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert!(
        items.is_empty(),
        "one close removes the item's only tab — there is no hidden twin left behind"
    );
    assert_eq!(active, None, "the pane holds no active item");
}
