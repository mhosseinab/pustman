//! The history-panel wire (S35w): the app's init order must register the History panel
//! in every workspace window's left dock once the local store it reads through exists,
//! and `history::Focus` must put the keyboard in it (PRD REQ-HIST-01, §8.5).

use gpui::TestAppContext;
use history_panel::{HistoryList, history_list::PANEL_ID};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::history::Focus;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_history_panel_registered_in_left_dock(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task and the panel registers once both the store
    // and the window exist, whichever arrives first.
    cx.run_until_parked();
    draw_window(cx);

    let registered: Vec<&'static str> = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panels()
                .iter()
                .map(|panel| panel.panel_id(cx))
                .collect()
        })
    });
    assert!(
        registered.contains(&PANEL_ID),
        "the left dock holds the History panel once the store and the window both exist \
         (REQ-HIST-01); registered panels: {registered:?}"
    );

    // `history::Focus` (D10) puts the keyboard in the panel. With one panel in the dock
    // its active-panel state is set at registration and is not proof of the action, so
    // the focused handle is the observable to assert here.
    cx.dispatch_action(Focus);
    cx.run_until_parked();

    let panel_focus = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panels()
                .iter()
                .find(|panel| panel.panel_id(cx) == PANEL_ID)
                .map(|panel| panel.focus_handle(cx))
        })
    });
    let panel_focus = panel_focus.expect("the history panel is registered (checked above)");
    cx.update(|window, _cx| {
        assert!(
            panel_focus.is_focused(window),
            "history::Focus moves keyboard focus into the history panel (PRD §8.5)"
        );
    });

    let active_is_history = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panel()
                .and_then(|view| view.downcast::<HistoryList>().ok())
                .is_some()
        })
    });
    assert!(
        active_is_history,
        "the left dock's active panel is the history panel after history::Focus"
    );
}
