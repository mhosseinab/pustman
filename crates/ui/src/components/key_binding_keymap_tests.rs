//! [`KeyBinding::from_keymap`]: a hint that reads the whole keymap shows its keys even
//! where the action's key context is not on the focus path — an empty state's shortcut
//! chip while an application menu has the keyboard (#332).

use gpui::{Context, IntoElement, Modifiers, Render, TestAppContext, Window, div};

use super::{KeyBinding, keystroke_label};
use crate::test_support::{draw_window, init_test};

gpui::actions!(key_binding_keymap_tests, [OpenThing]);

struct Empty;

impl Render for Empty {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn test_a_keymap_hint_shows_its_keys_outside_the_actions_context(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "ctrl-t",
            OpenThing,
            Some("SomewhereElse"),
        )]);
    });
    let (_view, cx) = cx.add_window_view(|_window, _cx| Empty);
    draw_window(cx);

    let (focused_keys, keymap_keys, expected_keys) = cx.update(|window, cx| {
        (
            KeyBinding::for_action(&OpenThing).keystrokes(window, cx),
            KeyBinding::from_keymap(&OpenThing).keystrokes(window, cx),
            // The platform's own spelling of ctrl-t: `Ctrl+T` on Linux, `^T` on macOS.
            vec![keystroke_label(
                &Modifiers::control(),
                "t",
                cx.platform_info().os,
            )],
        )
    });
    assert!(
        focused_keys.is_empty(),
        "the focus-scoped hint is blank where the action's context is not on the path"
    );
    assert_eq!(
        keymap_keys, expected_keys,
        "the keymap hint shows the keys wherever the focus is"
    );
}
