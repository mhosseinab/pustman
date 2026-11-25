//! The application title bar (S48u2): always on screen — a server-decorated window gets
//! it too — with the in-window menu bar drawn from the app's one menu model
//! (`cx.set_menus`), opened by a click or Alt+letter, walked with the arrows, run with a
//! click or Enter and closed with Escape. An entry whose action has no handler where the
//! user was is disabled, never a dead click.
//!
//! The model here is a small stand-in installed through `install_app_menus`, exactly
//! where the app installs its real one (`pustman::app_menus`); the app's own model is pinned
//! by that crate's tests.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Entity, Menu, MenuItem, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::request::Send;
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace, ToggleConsole};
use ui::components::context_menu::{HINT_SUFFIX, MENU_SELECTOR};
use ui::test_support::{draw_window, init_test, press_key};

use crate::title_bar::{
    APP_TITLE_BAR_SELECTOR, SEARCH_SELECTOR, SWITCHER_SELECTOR, TITLE_BAR_HEIGHT_PX,
    TITLE_BAR_SELECTOR, menu_title_selector,
};
use crate::{Workspace, init, install_app_menus};

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// The stand-in model: a File menu with the two workspace verbs and a Request menu whose
/// Send has no handler in a bare shell, followed by one that has.
fn test_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("New workspace…", CreateWorkspace),
            MenuItem::action("Open workspace…", OpenWorkspace),
        ]),
        Menu::new("Request").items([
            MenuItem::action("Send", Send),
            MenuItem::separator(),
            MenuItem::action("Toggle console", ToggleConsole),
        ]),
    ]
}

fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        install_app_menus(test_menus(), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn menu_is_open(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(MENU_SELECTOR).is_some()
}

#[gpui::test]
fn test_a_server_decorated_window_still_draws_the_app_title_bar_with_menus_switcher_and_search(
    cx: &mut TestAppContext,
) {
    let (_workspace, cx) = workspace_window(cx);

    let bar = cx
        .debug_bounds(APP_TITLE_BAR_SELECTOR)
        .expect("the app title bar draws whatever draws the window frame");
    assert_eq!(
        bar.size.height,
        gpui::px(TITLE_BAR_HEIGHT_PX),
        "the title bar is the blueprint's 44 px strip"
    );
    for selector in [
        leak(menu_title_selector("File")),
        leak(menu_title_selector("Request")),
        SWITCHER_SELECTOR,
        SEARCH_SELECTOR,
    ] {
        let control = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the title bar shows {selector}"));
        assert!(
            bar.contains(&control.center()),
            "{selector} sits in the bar"
        );
    }
    assert!(
        cx.debug_bounds(TITLE_BAR_SELECTOR).is_none(),
        "the window-frame title bar (controls, drag, resize) stays the compositor's"
    );
    assert!(!menu_is_open(cx), "no menu is open before the user asks");
}

#[gpui::test]
fn test_clicking_new_workspace_in_the_file_menu_dispatches_create_workspace(
    cx: &mut TestAppContext,
) {
    let (_workspace, cx) = workspace_window(cx);
    let created = Rc::new(Cell::new(0));
    cx.update(|_window, cx| {
        let created = created.clone();
        // Registered after `workspace::init`, so it runs first and stops the dialog.
        cx.on_action(move |_: &CreateWorkspace, _cx| created.set(created.get() + 1));
    });

    click(cx, leak(menu_title_selector("File")));
    assert!(menu_is_open(cx), "clicking File opens its menu");
    click(cx, "New workspace…");

    assert_eq!(created.get(), 1, "the entry ran its action exactly once");
    assert!(!menu_is_open(cx), "running an entry closes the menu");
}

#[gpui::test]
fn test_alt_f_opens_the_file_menu_and_escape_closes_it_returning_focus(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    cx.simulate_keystrokes("alt-f");
    cx.run_until_parked();
    draw_window(cx);
    assert!(menu_is_open(cx), "Alt+F opens a menu");
    assert!(
        cx.debug_bounds("New workspace…").is_some(),
        "the open menu is File"
    );

    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(!menu_is_open(cx), "Escape closes the menu");
    let focused = cx.update(|window, cx| {
        workspace.read(cx).focus_handle.contains_focused(window, cx)
            || workspace.read(cx).focus_handle.is_focused(window)
    });
    assert!(focused, "focus returns to where the user was");
}

#[gpui::test]
fn test_right_arrow_moves_from_the_open_menu_to_the_next_one(cx: &mut TestAppContext) {
    let (_workspace, cx) = workspace_window(cx);

    cx.simulate_keystrokes("alt-f");
    cx.run_until_parked();
    draw_window(cx);
    press_key(cx, "right");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds("Toggle console").is_some(),
        "Right opens the next menu, Request"
    );
    assert!(
        cx.debug_bounds("New workspace…").is_none(),
        "and closes File"
    );
}

#[gpui::test]
fn test_an_entry_without_a_handler_where_the_user_is_is_disabled(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    assert!(cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open()));

    cx.simulate_keystrokes("alt-r");
    cx.run_until_parked();
    draw_window(cx);
    // Send has no handler in a bare shell, so the highlight starts past it and Enter
    // runs the first entry that can run.
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !cx.read(|cx| workspace.read(cx).bottom_dock().read(cx).is_open()),
        "Enter skipped the disabled Send and ran Toggle console"
    );
}

#[gpui::test]
fn test_each_entry_shows_the_binding_the_keymap_gives_its_action(cx: &mut TestAppContext) {
    let (_workspace, cx) = workspace_window(cx);
    cx.update(|_window, cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "ctrl-shift-w",
            CreateWorkspace,
            Some("Workspace"),
        )]);
    });

    click(cx, leak(menu_title_selector("File")));

    assert!(
        cx.debug_bounds(leak(format!("New workspace…{HINT_SUFFIX}")))
            .is_some(),
        "a bound action shows its keys"
    );
    assert!(
        cx.debug_bounds(leak(format!("Open workspace…{HINT_SUFFIX}")))
            .is_none(),
        "an unbound action shows none"
    );
}
