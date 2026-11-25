//! Walking out of a submenu and away from the bar (#332): Left or Escape in File › Open
//! recent goes back to File (a second Escape closes it), and Tab — or the keyboard
//! leaving the menu any other way — closes an open popover instead of leaving it on
//! screen behind the focus.

use gpui::{Entity, Focusable, Menu, MenuItem, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::workspace::{OpenWorkspace, ToggleConsole};
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test};

use crate::title_bar::menu_title_selector;
use crate::{Workspace, init, install_app_menus};

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn test_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("Open workspace…", OpenWorkspace),
            MenuItem::submenu(
                Menu::new("Open recent").items([MenuItem::action("Toggle console", ToggleConsole)]),
            ),
        ]),
        Menu::new("View").items([MenuItem::action("Toggle console", ToggleConsole)]),
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

fn keys(cx: &mut VisualTestContext, keystrokes: &str) {
    cx.simulate_keystrokes(keystrokes);
    cx.run_until_parked();
    draw_window(cx);
}

fn menu_is_open(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(MENU_SELECTOR).is_some()
}

/// File › Open recent, opened with the mouse: the submenu is the open popover.
fn open_recent_submenu(cx: &mut VisualTestContext) {
    click(cx, leak(menu_title_selector("File")));
    click(cx, "Open recent");
    assert!(
        cx.debug_bounds("Open workspace…").is_none(),
        "the submenu replaced File in place"
    );
}

#[gpui::test]
fn test_left_in_open_recent_goes_back_to_the_file_menu(cx: &mut TestAppContext) {
    let (_workspace, cx) = workspace_window(cx);
    open_recent_submenu(cx);

    keys(cx, "left");

    assert!(menu_is_open(cx), "Left keeps a menu open");
    assert!(
        cx.debug_bounds("Open workspace…").is_some(),
        "Left in the submenu returns to File, not to the previous menu in the bar"
    );
    assert!(
        cx.debug_bounds("Open recent").is_some(),
        "File shows its submenu entry again"
    );
}

#[gpui::test]
fn test_escape_in_open_recent_goes_back_to_file_and_a_second_escape_closes_it(
    cx: &mut TestAppContext,
) {
    let (_workspace, cx) = workspace_window(cx);
    open_recent_submenu(cx);

    keys(cx, "escape");
    assert!(
        cx.debug_bounds("Open workspace…").is_some(),
        "the first Escape backs out of the submenu to File"
    );

    keys(cx, "escape");
    assert!(!menu_is_open(cx), "the second Escape closes File");
}

#[gpui::test]
fn test_escape_in_a_top_level_menu_still_closes_it(cx: &mut TestAppContext) {
    let (_workspace, cx) = workspace_window(cx);
    click(cx, leak(menu_title_selector("View")));
    assert!(menu_is_open(cx));
    keys(cx, "escape");
    assert!(!menu_is_open(cx), "a menu with no parent closes on Escape");
}

#[gpui::test]
fn test_tab_closes_an_open_menu_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    click(cx, leak(menu_title_selector("File")));
    assert!(menu_is_open(cx));

    keys(cx, "tab");

    assert!(!menu_is_open(cx), "Tab closes the open menu");
    let workspace_focused = cx.update(|window, cx| {
        workspace
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
    });
    assert!(
        workspace_focused,
        "the keyboard goes back to the working context"
    );
}

#[gpui::test]
fn test_focus_leaving_the_menu_closes_it(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    click(cx, leak(menu_title_selector("File")));
    assert!(menu_is_open(cx));

    // Something else takes the keyboard while the menu is open.
    cx.update(|window, cx| {
        let outside = workspace.read(cx).focus_handle(cx);
        window.focus(&outside, cx);
    });
    cx.run_until_parked();
    draw_window(cx);

    assert!(!menu_is_open(cx), "focus leaving the menu closes it");
}
