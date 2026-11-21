//! Starting work from the Welcome tab in the full app: `Ctrl-T` (`workspace::NewTab`) and
//! the Welcome screen's "New Request" button both open a request tab, never another
//! Welcome tab (PRD REQ-UI-02: one tab per open request).

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use pustman::StoreLocation;
use pustman_actions::workspace::NewTab;
use request_editor::RequestEditor;
use ui::test_support::draw_window;
use workspace::Workspace;

fn app_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

fn active_tab_is_request_editor(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .is_some_and(|item| item.to_any_view().downcast::<RequestEditor>().is_ok())
        })
    })
}

#[gpui::test]
fn test_new_tab_opens_a_request_tab_in_the_app(cx: &mut TestAppContext) {
    let (workspace, cx) = app_window(cx);
    assert!(!active_tab_is_request_editor(&workspace, cx));

    cx.dispatch_action(NewTab);
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        active_tab_is_request_editor(&workspace, cx),
        "Ctrl-T opens a request the user can type a URL into, not another Welcome tab"
    );
}

#[gpui::test]
fn test_welcome_new_request_button_opens_a_request_tab(cx: &mut TestAppContext) {
    let (workspace, cx) = app_window(cx);
    let button = cx
        .debug_bounds("welcome-new-request")
        .expect("the Welcome screen shows a New Request button");

    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        active_tab_is_request_editor(&workspace, cx),
        "clicking New Request on the Welcome screen opens a request tab"
    );
}
