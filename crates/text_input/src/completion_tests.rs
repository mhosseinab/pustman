//! The completion trigger behind a field's autocomplete, driven the way a user drives
//! it: a real window with focus, the caret placed with the arrow keys, and the exact
//! template an accepted name leaves in the field — including with the caret in the
//! middle of a name being replaced, where the name's trailing characters must not
//! survive the insert.

use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::Completion;
use crate::init;
use crate::input::TextInput;

/// A window with one input holding `text`, drawn and focused, ready for keystrokes.
fn focused_input<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextInput>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new(window, cx);
        input.set_text(text, cx);
        input
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| input.read_with(cx, |input, cx| input.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (input, cx)
}

/// The field's text, read through the entity.
fn text_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    cx.read(|cx| input.read_with(cx, |input, _| input.text()))
}

/// The field's caret offset.
fn caret_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> usize {
    cx.read(|cx| input.read_with(cx, |input, _| input.selected_range().start))
}

/// The completion trigger at the caret, if one is open.
fn completion_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> Option<Completion> {
    cx.read(|cx| input.read_with(cx, |input, _| input.completion()))
}

/// Accept `name` into the open completion, the way the popover's Enter does.
fn accept(
    input: &Entity<TextInput>,
    completion: &Completion,
    name: &str,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.insert_completion(completion, name, window, cx)
        });
    });
}

#[gpui::test]
fn test_completing_mid_name_replaces_the_whole_name(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "{{token}}");

    // Five lefts walk the caret back into the name: `{{to|ken}}`.
    for _ in 0..5 {
        press_key(cx, "left");
    }
    assert_eq!(
        caret_of(&input, cx),
        4,
        "five lefts put the caret in the middle of the name"
    );

    let completion = completion_of(&input, cx).expect("`{{` before the caret opens a completion");
    assert_eq!(
        completion.query(),
        "to",
        "the query is the partial name before the caret"
    );
    assert_eq!(
        completion.replace_range(),
        &(2..7),
        "the replace range covers the name on both sides of the caret"
    );

    accept(&input, &completion, "token", cx);
    assert_eq!(
        text_of(&input, cx),
        "{{token}}",
        "accepting a name mid-name replaces the whole name and closes it once"
    );
    assert_eq!(
        caret_of(&input, cx),
        9,
        "the caret lands after the placeholder's braces"
    );
}

#[gpui::test]
fn test_completing_before_closing_braces_writes_no_second_braces(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "{{token}}");

    // Two lefts put the caret between the name and the closing braces: `{{token|}}`.
    press_key(cx, "left");
    press_key(cx, "left");
    assert_eq!(caret_of(&input, cx), 7);

    let completion = completion_of(&input, cx).expect("`{{` before the caret opens a completion");
    assert_eq!(completion.query(), "token");
    accept(&input, &completion, "token", cx);

    assert_eq!(
        text_of(&input, cx),
        "{{token}}",
        "the braces already behind the name are reused, not written twice"
    );
    assert_eq!(caret_of(&input, cx), 9);
}
