//! The left dock's icon rail (S48u3): one entry per panel left of the sidebar, the
//! active one marked; a click or the keyboard (Tab to the entry, Enter) switches the
//! sidebar to that panel and moves focus into it, and the rail stays while the sidebar
//! is hidden so a click brings it back. Help at the bottom opens the command palette.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Entity, FocusHandle, Focusable, Modifiers, TestAppContext, VisualTestContext, Window};
use pustman_actions::palette;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

use crate::dock::{RAIL_HELP_ID, dock_rail_selector};
use crate::{DockPosition, Panel, Workspace, init};

struct TestPanel {
    focus_handle: FocusHandle,
    panel_id: &'static str,
}

impl Render for TestPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .child(Label::new(self.panel_id))
    }
}

impl Focusable for TestPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for TestPanel {
    fn panel_id(&self) -> &'static str {
        self.panel_id
    }

    fn title(&self, _cx: &App) -> SharedString {
        self.panel_id.into()
    }

    fn icon(&self) -> IconName {
        IconName::Folder
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Left
    }
}

/// A workspace with three left-dock panels registered in the rail's order.
fn workspace_with_panels(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Vec<Entity<TestPanel>>,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let mut panels = Vec::new();
    for panel_id in ["collections", "environments", "history"] {
        let panel = cx.update(|_window, cx| {
            cx.new(|cx| TestPanel {
                focus_handle: cx.focus_handle(),
                panel_id,
            })
        });
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.register_panel(panel.clone(), window, cx)
            });
        });
        panels.push(panel);
    }
    draw_window(cx);
    (workspace, panels, cx)
}

fn active_panel(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<&'static str> {
    cx.update(|_window, cx| {
        let dock = workspace.read(cx).left_dock().read(cx);
        dock.is_open().then(|| dock.active_panel_id(cx)).flatten()
    })
}

fn rail_bounds(cx: &mut VisualTestContext, panel_id: &str) -> gpui::Bounds<Pixels> {
    let selector: &'static str = dock_rail_selector(panel_id).leak();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("the {panel_id} rail entry renders"))
}

fn is_focused(panel: &Entity<TestPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.read(cx).focus_handle.is_focused(window))
}

#[gpui::test]
fn test_the_rail_sits_left_of_the_sidebar_and_a_click_switches_the_view(cx: &mut TestAppContext) {
    let (workspace, panels, cx) = workspace_with_panels(cx);
    let dock = cx.debug_bounds("left-dock").expect("the sidebar renders");
    let collections = rail_bounds(cx, "collections");
    let environments = rail_bounds(cx, "environments");
    let history = rail_bounds(cx, "history");
    assert!(
        collections.origin.x + collections.size.width <= dock.origin.x,
        "the rail sits left of the sidebar"
    );
    assert!(
        collections.origin.y < environments.origin.y && environments.origin.y < history.origin.y,
        "the entries stack top to bottom in registration order"
    );

    cx.simulate_click(environments.center(), Modifiers::none());
    draw_window(cx);
    assert_eq!(active_panel(&workspace, cx), Some("environments"));
    assert!(
        is_focused(&panels[1], cx),
        "the keyboard follows into the panel"
    );
}

#[gpui::test]
fn test_the_rail_is_keyboard_reachable_and_enter_switches_the_view(cx: &mut TestAppContext) {
    let (workspace, panels, cx) = workspace_with_panels(cx);
    // Tab walks the focusable controls in order until it lands on the History entry.
    let mut reached = false;
    for _ in 0..40 {
        cx.update(|window, cx| window.focus_next(cx));
        draw_window(cx);
        let on_history = cx.update(|window, cx| {
            workspace
                .read(cx)
                .left_dock()
                .read(cx)
                .rail_focus_handle("history")
                .is_some_and(|handle| handle.is_focused(window))
        });
        if on_history {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab reaches the History rail entry");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(active_panel(&workspace, cx), Some("history"));
    assert!(
        is_focused(&panels[2], cx),
        "Enter moves the keyboard into the panel"
    );
}

#[gpui::test]
fn test_the_rail_stays_while_the_sidebar_is_hidden_and_brings_it_back(cx: &mut TestAppContext) {
    let (workspace, _panels, cx) = workspace_with_panels(cx);
    cx.update(|_window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .left_dock()
                .update(cx, |dock, cx| dock.set_open(false, cx));
        });
    });
    draw_window(cx);
    assert!(
        cx.debug_bounds("left-dock").is_none(),
        "the sidebar is hidden"
    );
    let history = rail_bounds(cx, "history");
    cx.simulate_click(history.center(), Modifiers::none());
    draw_window(cx);
    assert_eq!(
        active_panel(&workspace, cx),
        Some("history"),
        "a rail click shows the sidebar on that panel"
    );
}

#[gpui::test]
fn test_rail_help_opens_the_command_palette(cx: &mut TestAppContext) {
    let (_workspace, _panels, cx) = workspace_with_panels(cx);
    let toggles = Rc::new(Cell::new(0));
    cx.update(|_window, cx| {
        let toggles = toggles.clone();
        cx.on_action(move |_: &palette::Toggle, _cx| toggles.set(toggles.get() + 1));
    });
    let help = cx
        .debug_bounds(RAIL_HELP_ID)
        .expect("Help renders on the rail");
    cx.simulate_click(help.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(toggles.get(), 1, "Help dispatches palette::Toggle");
}
