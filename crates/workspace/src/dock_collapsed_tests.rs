//! The collapsed console strip (S48u5): a collapsed bottom dock draws only its header
//! row — a one-line strip, not a 200 px empty pane — and `ToggleConsole` or the strip's
//! expand action unfold it to its full height. A layout saved collapsed restores
//! collapsed (`restore.rs`); the app shell starts every window that way
//! (`pustman::status_items`).

use gpui::{Entity, FocusHandle, Focusable, TestAppContext, VisualTestContext};
use pustman_actions::workspace::ToggleConsole;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::dock::{dock_expand_selector, dock_header_selector, dock_hide_selector};
use crate::{DockPosition, Panel, Workspace, init};

/// The console's stand-in: a panel whose body is a selector the strip must not show.
struct ConsoleStandIn {
    focus_handle: FocusHandle,
}

const CONSOLE_BODY_SELECTOR: &str = "console-stand-in-body";

impl Render for ConsoleStandIn {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .debug_selector(|| CONSOLE_BODY_SELECTOR.to_owned())
            .child(Label::new("No console entries"))
    }
}

impl Focusable for ConsoleStandIn {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ConsoleStandIn {
    fn panel_id(&self) -> &'static str {
        "console"
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Console")
    }

    fn icon(&self) -> IconName {
        IconName::Terminal
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Bottom
    }
}

/// A workspace on the real init order and default keymap, holding the console stand-in,
/// with its bottom dock collapsed the way the app starts it.
fn collapsed_console(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap binds: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let panel = cx.update(|_, cx| {
        cx.new(|cx| ConsoleStandIn {
            focus_handle: cx.focus_handle(),
        })
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(panel, window, cx);
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.set_collapsed(true, cx));
        });
    });
    cx.update(|window, cx| {
        let focus = workspace.focus_handle(cx);
        window.focus(&focus, cx);
    });
    draw_window(cx);
    (workspace, cx)
}

fn leak(selector: String) -> &'static str {
    selector.leak()
}

fn is_collapsed(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_collapsed())
}

#[gpui::test]
fn test_a_collapsed_console_draws_only_its_one_line_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);

    let dock = cx
        .debug_bounds("bottom-dock")
        .expect("the collapsed console still draws its strip");
    let header = cx
        .debug_bounds(leak(dock_header_selector(DockPosition::Bottom)))
        .expect("the strip is the panel header row");
    assert!(
        dock.size.height <= header.size.height + px(2.),
        "the strip is one header row tall ({dock:?}), not the 200 px pane"
    );
    assert!(
        cx.debug_bounds(CONSOLE_BODY_SELECTOR).is_none(),
        "the console body is not drawn while collapsed"
    );
    assert!(
        cx.debug_bounds(leak(dock_expand_selector(DockPosition::Bottom)))
            .is_some(),
        "the strip offers an expand action"
    );
    assert!(
        cx.debug_bounds(leak(dock_hide_selector(DockPosition::Bottom)))
            .is_none(),
        "the strip's action expands; hiding belongs to the open console"
    );
    assert!(
        cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open()),
        "a collapsed dock is still on screen"
    );
}

#[gpui::test]
fn test_toggle_console_unfolds_a_collapsed_console_and_then_hides_it(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);

    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    assert!(
        !is_collapsed(&workspace, cx),
        "ToggleConsole unfolds the strip"
    );
    assert!(
        cx.debug_bounds(CONSOLE_BODY_SELECTOR).is_some(),
        "the unfolded console shows its body"
    );

    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    assert!(
        !cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open()),
        "a second ToggleConsole hides the open console"
    );

    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    assert!(
        cx.debug_bounds(CONSOLE_BODY_SELECTOR).is_some(),
        "the third brings it back open, not as a strip"
    );
}

#[gpui::test]
fn test_clicking_the_strips_expand_action_unfolds_the_console(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    let expand = cx
        .debug_bounds(leak(dock_expand_selector(DockPosition::Bottom)))
        .expect("the expand action renders");

    cx.simulate_click(expand.center(), gpui::Modifiers::none());
    draw_window(cx);

    assert!(!is_collapsed(&workspace, cx), "the click unfolds the strip");
    assert!(
        cx.debug_bounds(CONSOLE_BODY_SELECTOR).is_some(),
        "the unfolded console shows its body"
    );
}
