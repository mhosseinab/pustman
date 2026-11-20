//! The command palette wire (S24w): the app's init order must plug `command_palette` in,
//! so `palette::Toggle` opens the palette over the workspace window — and the default
//! keymap must give that action the platform's `Ctrl/Cmd-Shift-P` (PRD §3, REQ-UI-05).

use command_palette::Picker;
use command_palette::palette::PaletteDelegate;
use gpui::{Keystroke, OperatingSystem, TestAppContext};
use pustman::StoreLocation;
use pustman_actions::palette::Toggle;
use ui::KeyBinding;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_command_palette_toggle_opens_modal(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task; let it land before the window draws.
    cx.run_until_parked();
    draw_window(cx);

    // The default keymap gives the toggle PRD §3's Ctrl/Cmd-Shift-P on this platform.
    let os = cx.read(|cx| cx.platform_info().os);
    let expected_keys = if os == OperatingSystem::Mac {
        "cmd-shift-p"
    } else {
        "ctrl-shift-p"
    };
    let expected_keystroke = Keystroke::parse_with_os(expected_keys, os)
        .expect("the platform toggle keystroke must parse");
    let expected_text =
        ui::keystroke_label(&expected_keystroke.modifiers, &expected_keystroke.key, os);
    let bound = cx.update(|window, cx| KeyBinding::for_action(&Toggle).keystroke_text(window, cx));
    assert_eq!(
        bound.as_deref(),
        Some(expected_text.as_str()),
        "the default keymap binds palette::Toggle to {expected_keys}"
    );

    cx.dispatch_action(Toggle);
    cx.run_until_parked();

    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            assert!(
                workspace
                    .modal_layer()
                    .active_modal::<Picker<PaletteDelegate>>()
                    .is_some(),
                "dispatching palette::Toggle opens the command palette in the workspace's \
                 modal layer (REQ-UI-05)"
            );
        })
    });
}
