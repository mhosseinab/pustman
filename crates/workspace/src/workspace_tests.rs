//! The workspace shell (PRD §3): both docks are open when a workspace opens, the status
//! bar carries the items other crates add to it, and [`ToggleSidebar`] / `ToggleConsole`
//! hide and show their docks — dispatched as real actions through the focused workspace,
//! the way the keymap delivers them. Calling `Dock::toggle` directly would test nothing.

use gpui::{
    Bounds, Entity, EntityId, Focusable, OperatingSystem, Pixels, TestAppContext, VisualTestContext,
};
use pustman_actions::workspace::ToggleSidebar;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

use crate::status_bar::{LEFT_ITEMS_SELECTOR, RIGHT_ITEMS_SELECTOR, status_item_selector};
use crate::{Workspace, init};

/// A status bar item: the shape the environment picker and the console add in later steps.
struct StatusItem {
    label: &'static str,
}

impl Render for StatusItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child(Label::new(self.label))
    }
}

/// A workspace in its own window, on the app's real init order and the real default
/// keymap, so actions reach it the way they do in the app.
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

/// Whether the left and the bottom dock are open, as the workspace sees them.
fn dock_states(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> (bool, bool) {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            (
                workspace.left_dock().read(cx).is_open(),
                workspace.bottom_dock().read(cx).is_open(),
            )
        })
    })
}

/// The rendered bounds of one status item, found under the selector its render puts on
/// it. `VisualTestContext::debug_bounds` takes a `&'static str`, so the two small
/// selector strings are leaked — fine for a test's short life.
fn status_item_bounds(cx: &mut VisualTestContext, item_id: EntityId) -> Option<Bounds<Pixels>> {
    let selector: &'static str = status_item_selector(item_id).leak();
    cx.debug_bounds(selector)
}

/// Whether `outer` fully contains `inner`.
fn contains_bounds(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height
}

#[gpui::test]
fn workspace_opens_with_docks_and_status_bar(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    let left_item = cx.update(|_, cx| {
        cx.new(|_| StatusItem {
            label: "environment",
        })
    });
    let left_item_id = left_item.entity_id();
    let right_item = cx.update(|_, cx| cx.new(|_| StatusItem { label: "cookies" }));
    let right_item_id = right_item.entity_id();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.status_bar().update(cx, |status_bar, cx| {
                status_bar.add_left_item(left_item, cx);
                status_bar.add_right_item(right_item, cx);
            });
        });
    });
    draw_window(cx);

    let (left_open, bottom_open) = dock_states(&workspace, cx);
    assert!(
        left_open,
        "the left dock is open when the workspace opens (REQ-UI-01)"
    );
    assert!(
        bottom_open,
        "the bottom dock is open when the workspace opens (REQ-UI-04)"
    );

    let (left_items, right_items) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let status_bar = workspace.status_bar().read(cx);
            (
                status_bar.left_items().len(),
                status_bar.right_items().len(),
            )
        })
    });
    assert_eq!(left_items, 1, "the status bar holds its one left item");
    assert_eq!(right_items, 1, "the status bar holds its one right item");

    assert!(
        cx.debug_bounds("left-dock").is_some(),
        "the open left dock renders"
    );
    assert!(
        cx.debug_bounds("bottom-dock").is_some(),
        "the open bottom dock renders"
    );
    assert!(
        cx.debug_bounds("status-bar").is_some(),
        "the status bar renders"
    );

    // Rendered-side proof: `debug_bounds` finds each status item under its
    // entity-derived selector — the left item inside the left group (the status bar's
    // left half), the right item inside the right group — so items dropped from the
    // render cannot pass here.
    let left_group = cx
        .debug_bounds(LEFT_ITEMS_SELECTOR)
        .expect("the left item group renders");
    let right_group = cx
        .debug_bounds(RIGHT_ITEMS_SELECTOR)
        .expect("the right item group renders");
    let left_bounds = status_item_bounds(cx, left_item_id).expect("the left status item renders");
    let right_bounds =
        status_item_bounds(cx, right_item_id).expect("the right status item renders");
    assert!(
        contains_bounds(left_group, left_bounds),
        "the left status item renders inside the status bar's left half"
    );
    assert!(
        contains_bounds(right_group, right_bounds),
        "the right status item renders inside the status bar's right half"
    );
    assert!(
        left_bounds.origin.x < right_bounds.origin.x,
        "the left status item renders left of the right one"
    );
}

#[gpui::test]
fn toggle_sidebar_hides_left_dock(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    draw_window(cx);

    cx.dispatch_action(ToggleSidebar);
    draw_window(cx);
    let (left_open, bottom_open) = dock_states(&workspace, cx);
    assert!(!left_open, "ToggleSidebar hides the left dock");
    assert!(bottom_open, "the bottom dock is unaffected");
    assert!(
        cx.debug_bounds("left-dock").is_none(),
        "the closed left dock is not rendered at all"
    );

    cx.dispatch_action(ToggleSidebar);
    draw_window(cx);
    let (left_open, _) = dock_states(&workspace, cx);
    assert!(left_open, "ToggleSidebar shows the left dock again");
}

#[gpui::test]
fn pressing_the_toggle_console_key_hides_the_bottom_dock(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    draw_window(cx);
    // The PRD §3 binding: Ctrl-Alt-C, Cmd-Alt-C on macOS (assets/keymaps/).
    let toggle_console_key = if cx.read(|cx| cx.platform_info().os == OperatingSystem::Mac) {
        "cmd-alt-c"
    } else {
        "ctrl-alt-c"
    };

    press_key(cx, toggle_console_key);
    draw_window(cx);
    let (left_open, bottom_open) = dock_states(&workspace, cx);
    assert!(!bottom_open, "the ToggleConsole key hides the bottom dock");
    assert!(left_open, "the left dock is unaffected");
    assert!(
        cx.debug_bounds("bottom-dock").is_none(),
        "the closed bottom dock is not rendered at all"
    );

    press_key(cx, toggle_console_key);
    draw_window(cx);
    let (_, bottom_open) = dock_states(&workspace, cx);
    assert!(
        bottom_open,
        "the ToggleConsole key shows the bottom dock again"
    );
}

#[gpui::test]
fn test_new_workspace_takes_keyboard_focus(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    let focused = cx.update(|window, cx| {
        let focus_handle = workspace.read_with(cx, |workspace, cx| workspace.focus_handle(cx));
        focus_handle.is_focused(window)
    });
    assert!(
        focused,
        "a new Workspace takes keyboard focus so the shell shortcuts work before any click"
    );
}

#[gpui::test]
fn adding_an_item_makes_it_the_panes_active_item(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let item = cx.update(|_, cx| cx.new(|_| StatusItem { label: "request" }));
    let item_id = item.entity_id();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_item(item, cx));
        });
    });

    let active_item = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .map(|item| item.entity_id())
        })
    });
    assert_eq!(
        active_item,
        Some(item_id),
        "the first item a pane holds becomes its active item"
    );
}
