//! `NewRequest` opens a request tab: the action is dispatched the way the app
//! dispatches it — against a real [`Workspace`] in its own window — and the pane is
//! asked what it holds afterwards.

use gpui::TestAppContext;
use pustman_actions::workspace::NewRequest;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use crate::RequestEditor;

#[gpui::test]
fn new_request_action_opens_tab(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);

    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    draw_window(cx);

    let (tab_count, active_is_editor) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.item_ids().len(),
                pane.active_item()
                    .map(|item| item.to_any_view().downcast::<RequestEditor>().is_ok()),
            )
        })
    });

    assert_eq!(tab_count, 1, "NewRequest opens one tab in the center pane");
    assert_eq!(
        active_is_editor,
        Some(true),
        "the tab NewRequest opened is a RequestEditor item"
    );
}
