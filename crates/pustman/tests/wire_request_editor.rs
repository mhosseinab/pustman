//! The request editor wire (S26w): the app's init order must plug `request_editor` in
//! and install the HTTP client, so `workspace::NewRequest` opens a request tab in the
//! workspace's center pane — the tab a real send starts from (PRD REQ-UI-02/03).

use gpui::TestAppContext;
use pustman::StoreLocation;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_request_editor_new_request_opens_tab(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task; let it land before the window draws.
    cx.run_until_parked();
    draw_window(cx);

    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    draw_window(cx);

    let active_is_editor = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .map(|item| item.to_any_view().downcast::<RequestEditor>().is_ok())
        })
    });
    assert_eq!(
        active_is_editor,
        Some(true),
        "dispatching workspace::NewRequest opens a RequestEditor tab in the workspace's \
         center pane (REQ-UI-02/03)"
    );
}
