//! The pane's tab strip (REQ-UI-02): tabs close, cycle, pin, reorder and carry the dirty
//! dot — and `NewTab`, `CloseTab`, `NextTab` and `PreviousTab` reach the pane as real
//! actions through the focused workspace, the way the keymap delivers them. Calling
//! `Pane` methods directly would test nothing about the dispatch path, so the two named
//! behaviours go through `dispatch_action`.

use gpui::{
    App, Context, Entity, EntityId, IntoElement, Modifiers, ParentElement, Render, SharedString,
    TestAppContext, VisualTestContext, Window,
};
use pustman_actions::workspace::{CloseTab, NewTab, NextTab, PreviousTab};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::item::Item;
use crate::{Workspace, init};

/// An item whose title and dirty state the test controls.
struct TestItem {
    title: &'static str,
    dirty: bool,
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

    fn is_dirty(&self, _cx: &App) -> bool {
        self.dirty
    }
}

/// A workspace in its own window, on the app's real init order and the real default
/// keymap, so the dispatched actions reach it the way the keymap delivers them.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    cx.add_window_view(Workspace::new)
}

/// Add one [`TestItem`] per title to the center pane as tabs, and return their ids.
fn add_items(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    titles: &[&'static str],
) -> Vec<EntityId> {
    cx.update(|_, cx| {
        let mut ids = Vec::new();
        for title in titles {
            let item = cx.new(|_| TestItem {
                title,
                dirty: false,
            });
            ids.push(item.entity_id());
            workspace.update(cx, |workspace, cx| {
                workspace
                    .center_pane()
                    .update(cx, |pane, cx| pane.add_tab_item(item, cx));
            });
        }
        ids
    })
}

/// The open items and the active one, as the pane sees them.
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

/// Which tab is showing, as its index in the tab order.
fn active_index(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> usize {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.center_pane().read(cx).active_index()
        })
    })
}

/// The debug selector the tab strip puts on one item's tab (`pane_tabs::rebuild_tabs`).
/// `debug_bounds` takes a `&'static str`, so the built string is leaked — fine for a
/// test's short life.
fn tab_selector(entity_id: EntityId) -> &'static str {
    format!("pane-tab-{entity_id}").leak()
}

/// The debug selector `ui::Tab` puts on one tab's close button.
fn tab_close_selector(entity_id: EntityId) -> &'static str {
    format!("pane-tab-{entity_id}-close").leak()
}

#[gpui::test]
fn close_tab_action_closes_active_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let ids = add_items(&workspace, cx, &["first", "second", "third"]);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.set_active_index(1, cx));
        });
    });

    cx.dispatch_action(CloseTab);
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(
        items,
        vec![ids[0], ids[2]],
        "the middle tab is closed and the other two remain"
    );
    assert_eq!(
        active,
        Some(ids[2]),
        "closing the active tab activates its right neighbour"
    );
}

#[gpui::test]
fn next_tab_cycles(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    add_items(&workspace, cx, &["first", "second", "third"]);
    // Adding a tab activates it, so step back to the first one first.
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.set_active_index(0, cx));
        });
    });

    for expected in [1, 2, 0] {
        cx.dispatch_action(NextTab);
        draw_window(cx);
        assert_eq!(
            active_index(&workspace, cx),
            expected,
            "NextTab moves through the tabs and wraps back to the first"
        );
    }
}

#[gpui::test]
fn previous_tab_wraps_to_the_last_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    add_items(&workspace, cx, &["first", "second", "third"]);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.set_active_index(0, cx));
        });
    });

    cx.dispatch_action(PreviousTab);
    draw_window(cx);

    assert_eq!(
        active_index(&workspace, cx),
        2,
        "PreviousTab wraps from the first tab to the last"
    );
}

#[gpui::test]
fn closing_the_last_tab_shows_the_welcome_empty_state(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let ids = add_items(&workspace, cx, &["only"]);
    draw_window(cx);
    assert!(
        cx.debug_bounds(tab_selector(ids[0])).is_some(),
        "the open tab renders before it is closed"
    );

    cx.dispatch_action(CloseTab);
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert!(items.is_empty(), "the last tab is closed");
    assert_eq!(active, None, "the pane holds no active item");
    assert!(
        cx.debug_bounds("welcome").is_some(),
        "the Welcome empty state renders when the pane holds no items"
    );
    assert!(
        cx.debug_bounds(tab_selector(ids[0])).is_none(),
        "the closed tab is gone from the tab strip"
    );
}

#[gpui::test]
fn pinned_tabs_sort_first_and_survive_close_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let ids = add_items(&workspace, cx, &["first", "second", "third"]);
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.set_pinned(ids[2], true, cx));
        });
    });
    let (items, _) = pane_state(&workspace, cx);
    assert_eq!(
        items,
        vec![ids[2], ids[0], ids[1]],
        "pinning a tab sorts it before the unpinned ones"
    );

    cx.dispatch_action(CloseTab);
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(items.len(), 3, "CloseTab does nothing on a pinned tab");
    assert_eq!(active, Some(ids[2]), "the pinned tab stays the active one");
}

#[gpui::test]
fn move_item_reorders_tabs(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let ids = add_items(&workspace, cx, &["first", "second", "third"]);

    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.move_item(2, 0, cx));
        });
    });

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(
        items,
        vec![ids[2], ids[0], ids[1]],
        "move_item puts the third tab first"
    );
    assert_eq!(
        active,
        Some(ids[2]),
        "the active item follows its tab to its new place"
    );
}

#[gpui::test]
fn a_dirty_items_tab_shows_the_dirty_dot(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let item = cx.update(|_, cx| {
        cx.new(|_| TestItem {
            title: "draft",
            dirty: false,
        })
    });
    let item_id = item.entity_id();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(item.clone(), cx));
        });
    });
    draw_window(cx);
    let clean = cx
        .debug_bounds(tab_selector(item_id))
        .expect("the clean tab rendered");

    cx.update(|_, cx| {
        item.update(cx, |item, cx| {
            item.dirty = true;
            cx.notify();
        });
    });
    draw_window(cx);

    let dirty = cx
        .debug_bounds(tab_selector(item_id))
        .expect("the dirty tab rendered");
    assert!(
        dirty.size.width > clean.size.width,
        "the dirty tab is wider than the clean one — the unsaved-changes dot is on it \
         ({dirty:?} vs {clean:?})"
    );
}

#[gpui::test]
fn new_tab_action_opens_a_welcome_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    add_items(&workspace, cx, &["first"]);

    cx.dispatch_action(NewTab);
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(items.len(), 2, "NewTab opens a second tab");
    assert_eq!(
        active,
        items.get(1).copied(),
        "the new tab is the active one"
    );
    let title = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .map(|item| item.tab_title(cx))
        })
    });
    assert_eq!(
        title.as_deref(),
        Some("Welcome"),
        "the new tab is a Welcome item until the request editor lands"
    );
}

#[gpui::test]
fn clicking_a_tabs_close_button_closes_that_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let ids = add_items(&workspace, cx, &["first", "second"]);
    draw_window(cx);

    let close_button = cx
        .debug_bounds(tab_close_selector(ids[0]))
        .expect("the first tab shows its close button");
    cx.simulate_click(close_button.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let (items, active) = pane_state(&workspace, cx);
    assert_eq!(
        items,
        vec![ids[1]],
        "clicking a tab's close button closes that tab"
    );
    assert_eq!(active, Some(ids[1]), "the remaining tab becomes active");
}
