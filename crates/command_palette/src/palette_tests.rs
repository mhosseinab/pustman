//! The palette is driven the way a user drives it: `palette::Toggle`'s production
//! keybinding, a query typed into the focused field, and Enter. Every assertion is an
//! outcome the user can see — every listed action with its humanized name and keybinding
//! hint, and the chosen action actually reaching a handler — so a palette that lists
//! nothing or never dispatches fails here.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Entity, KeyBinding, Keystroke, OperatingSystem, TestAppContext, VisualTestContext,
};
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use crate::picker::Picker;

use super::PaletteDelegate;

gpui::actions!(palette_test, [TestAlpha, TestBeta]);

/// A workspace window on the app's real init order and the real default keymap, so the
/// palette opens on the production `palette::Toggle` binding. The test actions are bound
/// and handled the way a host crate's actions are — an action with no handler is not
/// dispatchable and would never be listed — and `dispatches` counts their dispatches.
fn palette_window(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, Rc<Cell<usize>>, &mut VisualTestContext) {
    cx.update(init_test);
    let dispatches = Rc::new(Cell::new(0));
    cx.update({
        let dispatches = dispatches.clone();
        move |cx| {
            crate::init(cx);
            let problems = pustman_actions::load_default_keymap(cx);
            assert!(
                problems.is_empty(),
                "the default keymap must bind every entry: {problems:?}"
            );
            cx.bind_keys([
                KeyBinding::new("ctrl-alt-a", TestAlpha, None),
                KeyBinding::new("ctrl-alt-b", TestBeta, None),
            ]);
            cx.on_action({
                let dispatches = dispatches.clone();
                move |_: &TestAlpha, _cx: &mut App| dispatches.set(dispatches.get() + 1)
            });
            cx.on_action(move |_: &TestBeta, _cx: &mut App| dispatches.set(dispatches.get() + 1));
        }
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, dispatches, cx)
}

/// The production `palette::Toggle` keystroke on this platform — Cmd on macOS, Ctrl
/// elsewhere, exactly as `assets/keymaps/` binds it.
fn palette_toggle_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-shift-p"
    } else {
        "ctrl-shift-p"
    }
}

/// The text `ui::KeyBinding` shows for `keystroke` on this platform — what the palette
/// row of the action bound to it must show.
fn keystroke_text(keystroke: &str, cx: &VisualTestContext) -> String {
    let os = cx.read(|cx| cx.platform_info().os);
    let keystroke = Keystroke::parse_with_os(keystroke, os).expect("the test keystroke must parse");
    ui::keystroke_label(&keystroke.modifiers, &keystroke.key, os)
}

/// Whether the modal layer is showing the palette.
fn palette_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<Picker<PaletteDelegate>>()
                .is_some()
        })
    })
}

#[gpui::test]
fn palette_lists_every_registered_action_with_binding(cx: &mut TestAppContext) {
    let (workspace, _dispatches, cx) = palette_window(cx);

    press_key(cx, palette_toggle_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let listed: Vec<(String, Option<String>)> = cx.update(|window, cx| {
        let palette = workspace
            .read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<Picker<PaletteDelegate>>()
            })
            .expect("the palette is open after palette::Toggle");
        palette.read_with(cx, |picker, cx| {
            picker
                .delegate()
                .entries()
                .iter()
                .map(|entry| (entry.name().to_string(), entry.binding_text(window, cx)))
                .collect()
        })
    });

    assert!(
        listed.contains(&(
            "Palette Test: Test Alpha".to_string(),
            Some(keystroke_text("ctrl-alt-a", cx))
        )),
        "each test action is listed under its humanized name with its binding: {listed:?}"
    );
    assert!(
        listed.contains(&(
            "Palette Test: Test Beta".to_string(),
            Some(keystroke_text("ctrl-alt-b", cx))
        )),
        "each test action is listed under its humanized name with its binding: {listed:?}"
    );
    assert!(
        listed
            .iter()
            .any(|(name, _)| name == "Workspace: Toggle Console"),
        "the production actions come from the real keymap and handlers: {listed:?}"
    );
    assert!(
        !listed.iter().any(|(name, _)| name == "Palette: Toggle"),
        "the palette never offers itself: {listed:?}"
    );
}

#[gpui::test]
fn enter_dispatches_selected_action(cx: &mut TestAppContext) {
    let (workspace, dispatches, cx) = palette_window(cx);
    let toggle = palette_toggle_keys(cx);

    press_key(cx, toggle);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        palette_is_open(&workspace, cx),
        "palette::Toggle opens the palette"
    );

    cx.simulate_input("test alpha");
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        dispatches.get(),
        1,
        "enter dispatches the selected action and it reaches its handler"
    );
    assert!(
        !palette_is_open(&workspace, cx),
        "the palette closed and handed focus back"
    );

    // The same path delivers to the handlers on the workspace root: the action travels
    // from the focus the closed modal restored.
    press_key(cx, toggle);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input("toggle console");
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    let console_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.bottom_dock().read(cx).is_open()
        })
    });
    assert!(
        !console_open,
        "workspace::Toggle Console reached the workspace handler on the root"
    );
}
