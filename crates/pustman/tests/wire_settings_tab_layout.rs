//! Opening the request's Settings tab in the full app, at a laptop-sized window: the
//! form scrolls inside its tab and never pushes the pane's tab bar or the URL row up.

use gpui::{Modifiers, TestAppContext, VisualTestContext, px, size};
use pustman::StoreLocation;
use pustman_actions::workspace::NewTab;
use ui::test_support::draw_window;
use workspace::Workspace;

fn app_window(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.simulate_resize(size(px(1440.), px(900.)));
    cx.run_until_parked();
    draw_window(cx);
    cx
}

#[gpui::test]
fn test_opening_settings_keeps_the_url_row_and_tab_strip_in_place(cx: &mut TestAppContext) {
    let cx = app_window(cx);
    cx.dispatch_action(NewTab);
    cx.run_until_parked();
    draw_window(cx);
    let method_before = cx
        .debug_bounds("method-control")
        .expect("a new request tab shows the method control");
    let settings_tab = cx
        .debug_bounds("request-tab-settings")
        .expect("the request shows a Settings tab");

    cx.simulate_click(settings_tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.debug_bounds("method-control"),
        Some(method_before),
        "the Settings form scrolls inside its tab; it never pushes the URL row up"
    );
    assert_eq!(
        cx.debug_bounds("request-tab-settings")
            .map(|bounds| bounds.origin),
        Some(settings_tab.origin),
        "the request tab strip stays put when Settings opens"
    );
}
