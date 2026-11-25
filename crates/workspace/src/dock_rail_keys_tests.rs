//! The icon rail from the keyboard (#333): Up and Down move between its entries —
//! wrapping at the ends, without switching the view — and Enter or Space on the focused
//! entry switches the sidebar to it.

use gpui::{Entity, FocusHandle, Focusable, TestAppContext, VisualTestContext, Window};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

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

const PANELS: [&str; 3] = ["collections", "environments", "history"];

fn workspace_with_panels(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    for panel_id in PANELS {
        cx.update(|window, cx| {
            let panel = cx.new(|cx| TestPanel {
                focus_handle: cx.focus_handle(),
                panel_id,
            });
            workspace.update(cx, |workspace, cx| {
                workspace.register_panel(panel, window, cx)
            });
        });
    }
    draw_window(cx);
    (workspace, cx)
}

/// Put the keyboard on `panel_id`'s rail entry, as Tab would.
fn focus_rail_entry(workspace: &Entity<Workspace>, panel_id: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let handle = workspace
            .read(cx)
            .left_dock()
            .read(cx)
            .rail_focus_handle(panel_id)
            .cloned()
            .expect("the entry has a focus handle");
        window.focus(&handle, cx);
    });
    draw_window(cx);
}

/// The rail entry holding the keyboard, if any.
fn focused_rail_entry(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<&'static str> {
    cx.update(|window, cx| {
        let dock = workspace.read(cx).left_dock().read(cx);
        PANELS.into_iter().find(|panel_id| {
            dock.rail_focus_handle(panel_id)
                .is_some_and(|handle| handle.is_focused(window))
        })
    })
}

fn active_panel(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<&'static str> {
    cx.update(|_window, cx| workspace.read(cx).left_dock().read(cx).active_panel_id(cx))
}

fn key(cx: &mut VisualTestContext, keystroke: &str) {
    cx.simulate_keystrokes(keystroke);
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_down_and_up_move_between_rail_entries_without_switching_the_view(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_with_panels(cx);
    focus_rail_entry(&workspace, "collections", cx);

    key(cx, "down");
    assert_eq!(focused_rail_entry(&workspace, cx), Some("environments"));
    key(cx, "down");
    assert_eq!(focused_rail_entry(&workspace, cx), Some("history"));
    key(cx, "down");
    assert_eq!(
        focused_rail_entry(&workspace, cx),
        Some("collections"),
        "Down wraps from the last entry to the first"
    );
    key(cx, "up");
    assert_eq!(
        focused_rail_entry(&workspace, cx),
        Some("history"),
        "Up wraps from the first entry to the last"
    );
    assert_eq!(
        active_panel(&workspace, cx),
        Some("collections"),
        "moving along the rail does not switch the view"
    );
}

#[gpui::test]
fn test_enter_on_the_focused_rail_entry_switches_the_view(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_with_panels(cx);
    focus_rail_entry(&workspace, "collections", cx);
    key(cx, "down");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(active_panel(&workspace, cx), Some("environments"));
}

#[gpui::test]
fn test_space_on_the_focused_rail_entry_switches_the_view(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_with_panels(cx);
    focus_rail_entry(&workspace, "collections", cx);
    key(cx, "up");
    press_key(cx, "space");
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(active_panel(&workspace, cx), Some("history"));
}
