//! Palette rows list the most recently used actions first (pack S15/R15; the
//! `rust-gpui-ui-ux` skill's `postman-screens.md`: "Recently used actions first"):
//! confirming a row moves its action to the top of the palette's next rows, and the
//! actions never used keep their alphabetical order behind it.

use gpui::{App, Entity, KeyBinding, OperatingSystem, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use crate::picker::Picker;

use super::PaletteDelegate;

gpui::actions!(palette_recent_test, [TestAlpha, TestBeta, TestGamma]);

const ALPHA: &str = "Palette Recent Test: Test Alpha";
const BETA: &str = "Palette Recent Test: Test Beta";
const GAMMA: &str = "Palette Recent Test: Test Gamma";

/// A workspace window on the app's real init order and the real default keymap, with
/// three actions the palette can list and dispatch — an action with no handler is not
/// dispatchable and would never be listed.
fn palette_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        crate::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
        cx.bind_keys([
            KeyBinding::new("ctrl-alt-a", TestAlpha, None),
            KeyBinding::new("ctrl-alt-b", TestBeta, None),
            KeyBinding::new("ctrl-alt-c", TestGamma, None),
        ]);
        cx.on_action(|_: &TestAlpha, _cx: &mut App| {});
        cx.on_action(|_: &TestBeta, _cx: &mut App| {});
        cx.on_action(|_: &TestGamma, _cx: &mut App| {});
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
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

/// The names of the palette's rows, top first, as the user reads them. The palette is
/// open.
fn row_names(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        let palette = workspace
            .read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<Picker<PaletteDelegate>>()
            })
            .expect("the palette is open");
        palette.read_with(cx, |picker, _| {
            picker
                .delegate()
                .entries()
                .iter()
                .map(|entry| entry.name().to_string())
                .collect()
        })
    })
}

/// The test's own rows among `names`, top first — the palette also lists every other
/// registered action, which this ordering test does not care about.
fn test_rows(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.starts_with("Palette Recent Test: "))
        .cloned()
        .collect()
}

#[gpui::test]
fn confirming_a_palette_row_lists_its_action_first_the_next_time(cx: &mut TestAppContext) {
    let (workspace, cx) = palette_window(cx);
    let toggle = palette_toggle_keys(cx);

    press_key(cx, toggle);
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        test_rows(&row_names(&workspace, cx)),
        vec![ALPHA, BETA, GAMMA],
        "the first palette lists the never-used actions alphabetically"
    );

    cx.simulate_input("test beta");
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    press_key(cx, toggle);
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        test_rows(&row_names(&workspace, cx)),
        vec![BETA, ALPHA, GAMMA],
        "the confirmed action is listed first the next time; the unused ones stay alphabetical"
    );
}
