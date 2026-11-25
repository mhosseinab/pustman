//! The collapsed console strip (S48u5): a collapsed bottom dock draws only its header
//! row — a one-line strip, not a 200 px empty pane. A dock that rests as a strip (the
//! app shell's console, `pustman::status_items`) moves between strip and full height:
//! `ToggleConsole`, the strip's expand action and the open console's collapse action
//! never hide it. A saved layout wins over the resting default whatever the order.

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
/// with its bottom dock resting as the strip the way the app starts it.
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
                .update(cx, |dock, cx| dock.rest_as_strip(cx));
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
fn test_toggle_console_moves_between_the_strip_and_the_full_console(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    let is_open =
        |cx: &VisualTestContext| cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open());

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
        is_collapsed(&workspace, cx),
        "a second ToggleConsole folds it back"
    );
    assert!(
        is_open(cx),
        "folded, not hidden: the strip is the resting state"
    );
    assert!(
        cx.debug_bounds("bottom-dock").is_some()
            && cx.debug_bounds(CONSOLE_BODY_SELECTOR).is_none(),
        "the strip is on screen and the body is not"
    );

    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    assert!(
        !is_collapsed(&workspace, cx),
        "and a third unfolds it again"
    );
}

#[gpui::test]
fn test_the_open_consoles_header_action_folds_it_to_the_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    let fold = cx
        .debug_bounds(leak(dock_hide_selector(DockPosition::Bottom)))
        .expect("the open console's header carries its fold action");

    cx.simulate_click(fold.center(), gpui::Modifiers::none());
    draw_window(cx);

    assert!(
        is_collapsed(&workspace, cx),
        "the header action folds to the strip"
    );
    assert!(
        cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open()),
        "it does not hide the console"
    );
}

#[gpui::test]
fn test_folding_with_the_keyboard_in_the_console_moves_it_to_the_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    cx.dispatch_action(ToggleConsole);
    draw_window(cx);
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.activate_panel("console", window, cx));
        });
    });
    draw_window(cx);
    let panel_focused = cx.update(|window, cx| {
        let dock = workspace.read(cx).bottom_dock().read(cx);
        dock.panels()
            .first()
            .is_some_and(|panel| panel.focus_handle(cx).contains_focused(window, cx))
    });
    assert!(panel_focused, "the keyboard starts inside the console");

    cx.dispatch_action(ToggleConsole);
    draw_window(cx);

    let expand_focused = cx.update(|window, cx| {
        workspace
            .read(cx)
            .bottom_dock()
            .read(cx)
            .expand_focus_handle()
            .is_focused(window)
    });
    assert!(
        expand_focused,
        "folding the console hands the keyboard to the strip's expand button, not to nothing"
    );
}

#[gpui::test]
fn test_choosing_the_console_panel_unfolds_the_strip(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.activate_panel("console", window, cx));
        });
    });
    draw_window(cx);
    assert!(
        !is_collapsed(&workspace, cx),
        "activating moves the keyboard into the panel, so the panel must be on screen"
    );
}

#[gpui::test]
fn test_registering_a_panel_leaves_the_strip_folded(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    let second = cx.update(|_, cx| {
        cx.new(|cx| ConsoleStandIn {
            focus_handle: cx.focus_handle(),
        })
    });
    cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(second, window, cx)
        });
    });
    draw_window(cx);
    assert!(
        is_collapsed(&workspace, cx),
        "registration never unfolds the strip"
    );
}

#[gpui::test]
fn test_a_left_dock_ignores_collapsed(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    let before = cx.debug_bounds("left-dock").expect("the sidebar renders");
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.left_dock().update(cx, |dock, cx| {
                dock.set_collapsed(true, cx);
                dock.rest_as_strip(cx);
            });
        });
    });
    draw_window(cx);
    let collapsed = cx.read(|cx| workspace.read(cx).left_dock().read(cx).is_collapsed());
    assert!(!collapsed, "only the bottom dock folds to a strip");
    let after = cx
        .debug_bounds("left-dock")
        .expect("the sidebar still renders");
    assert_eq!(before.size, after.size, "at its full size");
}

#[gpui::test]
fn test_a_saved_layout_wins_over_the_resting_strip_in_either_order(cx: &mut TestAppContext) {
    let (workspace, cx) = collapsed_console(cx);
    let state = |cx: &VisualTestContext| {
        cx.read(|cx| {
            let dock = workspace.read(cx).bottom_dock().read(cx);
            (dock.is_open(), dock.is_collapsed())
        })
    };
    // Restore lands after the shell's default: the saved expanded console wins.
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.bottom_dock().update(cx, |dock, cx| {
                dock.apply_saved_layout(true, false, 200., cx)
            });
        });
    });
    assert_eq!(state(cx), (true, false), "the saved layout is applied");
    // The shell's default arrives late: it must not overwrite what was restored.
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.rest_as_strip(cx));
        });
    });
    assert_eq!(
        state(cx),
        (true, false),
        "the resting default never overrides a saved layout"
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
