//! An entry's hint — the key binding an application menu shows beside it — draws at the
//! end of its row, muted, and a disabled entry keeps its hint; the keymap lookup behind
//! those hints reads the whole keymap, not the focused element's contexts.

use gpui::{TestAppContext, VisualTestContext};

use crate::components::context_menu::{ContextMenu, HINT_SUFFIX};
use crate::components::key_binding::{keymap_binding_text, keystroke_label};
use crate::test_support::{draw_window, init_test};

gpui::actions!(hint_tests, [SaveThing, UnboundThing]);

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

#[gpui::test]
fn test_an_entry_with_a_hint_draws_it_after_its_label_in_the_same_row(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_menu, cx) = cx.add_window_view(|_, cx| {
        ContextMenu::new(cx)
            .entry("Save", |_, _, _| {})
            .hint("Ctrl+S")
            .disabled_entry("Send")
            .hint("Ctrl+Enter")
            .entry("Plain", |_, _, _| {})
    });
    let cx: &mut VisualTestContext = cx;
    draw_window(cx);

    let row = cx.debug_bounds("Save").expect("the entry draws");
    let hint = cx
        .debug_bounds(leak(format!("Save{HINT_SUFFIX}")))
        .expect("the entry's hint draws");
    assert!(
        row.contains(&hint.center()),
        "the hint sits inside its own entry's row"
    );
    assert!(
        hint.center().x > row.center().x,
        "the hint sits at the end of the row, after the label"
    );
    assert!(
        cx.debug_bounds(leak(format!("Send{HINT_SUFFIX}")))
            .is_some(),
        "a disabled entry still shows its binding"
    );
    assert!(
        cx.debug_bounds(leak(format!("Plain{HINT_SUFFIX}")))
            .is_none(),
        "an entry without a hint draws none"
    );
}

#[gpui::test]
fn test_keymap_binding_text_reads_a_binding_whatever_its_context(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "ctrl-shift-s",
            SaveThing,
            Some("SomeEditorNobodyFocuses"),
        )]);
    });
    let text = cx.update(|cx| keymap_binding_text(&SaveThing, cx));
    let os = cx.update(|cx| cx.platform_info().os);
    let modifiers = gpui::Modifiers {
        control: true,
        shift: true,
        ..gpui::Modifiers::default()
    };
    let expected = keystroke_label(&modifiers, "s", os);
    assert_eq!(text.as_deref(), Some(expected.as_str()));
    assert_eq!(
        cx.update(|cx| keymap_binding_text(&UnboundThing, cx)),
        None,
        "an action nothing binds has no hint"
    );
}
