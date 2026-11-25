//! The in-window menu bar draws the menu model cached when it was installed (#332), not
//! a fresh `cx.get_menus()` per frame — and a reinstalled model (the Open recent list
//! changed) redraws the titles from the new copy.

use gpui::{Menu, MenuItem, TestAppContext};
use pustman_actions::workspace::{OpenWorkspace, ToggleConsole};
use ui::test_support::{draw_window, init_test};

use crate::title_bar::menu_title_selector;
use crate::{Workspace, init, install_app_menus};

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn menus(names: &[&'static str]) -> Vec<Menu> {
    names
        .iter()
        .map(|name| Menu::new(*name).items([MenuItem::action("Open workspace…", OpenWorkspace)]))
        .collect()
}

#[gpui::test]
fn test_reinstalling_the_menu_model_redraws_the_titles_from_the_new_model(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        install_app_menus(menus(&["File", "Request"]), cx);
    });
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);
    assert!(
        cx.debug_bounds(leak(menu_title_selector("Request")))
            .is_some()
    );

    cx.update(|_window, cx| {
        install_app_menus(
            vec![
                Menu::new("File").items([MenuItem::action("Open workspace…", OpenWorkspace)]),
                Menu::new("View").items([MenuItem::action("Toggle console", ToggleConsole)]),
            ],
            cx,
        );
    });
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(leak(menu_title_selector("View"))).is_some(),
        "the reinstalled model's new menu is drawn"
    );
    assert!(
        cx.debug_bounds(leak(menu_title_selector("Request")))
            .is_none(),
        "the old model's menu is gone"
    );
}

#[gpui::test]
fn test_the_bar_draws_the_installed_model_even_when_the_platform_model_differs(
    cx: &mut TestAppContext,
) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        install_app_menus(menus(&["File", "Request"]), cx);
        // Something other than `install_app_menus` replaces the platform's model: the bar
        // keeps drawing what was installed, read once — never re-read per frame.
        cx.set_menus(menus(&["Other"]));
    });
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);
    assert!(cx.debug_bounds(leak(menu_title_selector("File"))).is_some());
    assert!(
        cx.debug_bounds(leak(menu_title_selector("Other")))
            .is_none()
    );
}
