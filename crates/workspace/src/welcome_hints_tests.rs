//! The welcome's key hints (New Request's Ctrl+T, the sidebar's Ctrl+\) stay on screen
//! while an application menu has the keyboard (#332): the binding is looked up in the
//! keymap, not under the focused element's key contexts.

use gpui::{Menu, MenuItem, Modifiers, TestAppContext, VisualTestContext, px};
use pustman_actions::workspace::{NewTab, OpenWorkspace, ToggleSidebar};
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test};

use crate::title_bar::menu_title_selector;
use crate::welcome::{NEW_REQUEST_KEYS_SELECTOR, TOGGLE_SIDEBAR_KEYS_SELECTOR};
use crate::{Workspace, init, install_app_menus};

fn hint_width(cx: &mut VisualTestContext, selector: &'static str) -> gpui::Pixels {
    cx.debug_bounds(selector)
        .map(|bounds| bounds.size.width)
        .unwrap_or_default()
}

#[gpui::test]
fn test_welcome_key_hints_stay_visible_while_a_menu_has_focus(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        // The shipped keymap's bindings (assets/keymaps), in the shell's context.
        cx.bind_keys([
            gpui::KeyBinding::new("ctrl-t", NewTab, Some(crate::WORKSPACE_CONTEXT)),
            gpui::KeyBinding::new("ctrl-\\", ToggleSidebar, Some(crate::WORKSPACE_CONTEXT)),
        ]);
        install_app_menus(
            vec![Menu::new("File").items([MenuItem::action("Open workspace…", OpenWorkspace)])],
            cx,
        );
    });
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    for selector in [NEW_REQUEST_KEYS_SELECTOR, TOGGLE_SIDEBAR_KEYS_SELECTOR] {
        assert!(
            hint_width(cx, selector) > px(0.),
            "{selector} shows its keys before any menu opens"
        );
    }

    let file = cx
        .debug_bounds(Box::leak(menu_title_selector("File").into_boxed_str()))
        .expect("the File title is on screen");
    cx.simulate_click(file.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    assert!(cx.debug_bounds(MENU_SELECTOR).is_some(), "File is open");

    for selector in [NEW_REQUEST_KEYS_SELECTOR, TOGGLE_SIDEBAR_KEYS_SELECTOR] {
        assert!(
            hint_width(cx, selector) > px(0.),
            "{selector} keeps its keys while the menu has the keyboard"
        );
    }
}
