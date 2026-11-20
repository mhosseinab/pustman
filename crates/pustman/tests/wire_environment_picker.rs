//! The environment picker wire (S34w): the app's init order must plug `environment_picker`
//! in and install its inputs, so `environment::SelectEnvironment` opens the picker over the
//! workspace window — not the crate's "not wired" error toast (PRD REQ-VAR-08).

use command_palette::Picker;
use environment_picker::EnvironmentSelector;
use gpui::TestAppContext;
use pustman::StoreLocation;
use pustman_actions::environment::SelectEnvironment;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_environment_picker_init_registers_actions(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task and the picker's inputs are installed when it
    // lands; let it land before the window draws.
    cx.run_until_parked();
    draw_window(cx);

    // The default keymap binds no keystroke to `environment::SelectEnvironment` yet, so
    // the action is dispatched the way a keystroke would.
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);

    let picker_is_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<Picker<EnvironmentSelector>>()
                .is_some()
        })
    });
    assert!(
        picker_is_open,
        "dispatching environment::SelectEnvironment opens the environment picker in the \
         workspace's modal layer, with the app's inputs installed (REQ-VAR-08)"
    );
}
