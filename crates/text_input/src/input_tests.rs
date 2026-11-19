//! The single-line input driven the way a user drives it: a real window with focus,
//! keystrokes through the keys [`crate::init`] binds, and typing through the
//! `EntityInputHandler` path `simulate_input` takes. The assertions are exact texts and
//! byte ranges, so a test fails when the behavior drifts and not only when it panics.

use std::ops::Range;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Context, Entity, Focusable, TestAppContext, VisualTestContext, Window};

use ui::test_support::{draw_window, init_test, press_key};

use super::{PLACEHOLDER_SELECTOR, TextInput, TextInputEvent};
use crate::init;

/// A window with one input holding `text`, drawn and focused, ready for keystrokes.
fn focused_input<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextInput>, &'a mut VisualTestContext) {
    open_input(cx, text, TextInput::new)
}

/// The same, with `build` shaping the field first (placeholder, read-only, …).
fn open_input<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    build: impl FnOnce(&mut Window, &mut Context<TextInput>) -> TextInput,
) -> (Entity<TextInput>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = build(window, cx);
        input.set_text(text, cx);
        input
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| input.read_with(cx, |input, cx| input.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (input, cx)
}

/// Every event the input has emitted since the last call.
fn drain(events: &mut UnboundedReceiver<TextInputEvent>) -> Vec<TextInputEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

/// The field's text, read through the entity.
fn text_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    cx.read(|cx| input.read_with(cx, |input, _| input.text()))
}

/// The field's selected byte range — an empty range is the caret.
fn selection_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> Range<usize> {
    cx.read(|cx| input.read_with(cx, |input, _| input.selected_range()))
}

/// The platform's word-left keystroke and its shift-selection twin.
fn word_keystrokes() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("alt-left", "alt-shift-left")
    } else {
        ("ctrl-left", "ctrl-shift-left")
    }
}

/// The platform's select-all keystroke.
fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[gpui::test]
fn typing_inserts_at_cursor(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");
    let mut events = cx.events(&input);

    cx.simulate_input("hello");
    press_key(cx, "left");
    press_key(cx, "left");
    cx.simulate_input("z");

    assert_eq!(text_of(&input, cx), "helzlo", "typing lands at the caret");
    assert_eq!(
        drain(&mut events),
        vec![TextInputEvent::Changed; 6],
        "every edit emits Changed exactly once"
    );
}

#[gpui::test]
fn word_motion_and_shift_selection(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "hello brave world");
    let (word_left, select_word_left) = word_keystrokes();

    cx.simulate_keystrokes(word_left);
    assert_eq!(
        selection_of(&input, cx),
        12..12,
        "word-left lands at the start of \"world\""
    );

    cx.simulate_keystrokes(select_word_left);
    assert_eq!(
        selection_of(&input, cx),
        6..12,
        "shift+word-left selects \"brave \""
    );

    cx.simulate_keystrokes(select_word_left);
    assert_eq!(
        selection_of(&input, cx),
        0..12,
        "shift+word-left again extends the selection to the text start"
    );
}

#[gpui::test]
fn enter_in_single_line_emits_submitted(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");
    let mut events = cx.events(&input);
    cx.simulate_input("hello");
    drain(&mut events);

    press_key(cx, "enter");

    assert_eq!(
        drain(&mut events),
        vec![TextInputEvent::Submitted],
        "Enter emits Submitted exactly once"
    );
    assert_eq!(
        text_of(&input, cx),
        "hello",
        "Enter never inserts a newline"
    );
}

#[gpui::test]
fn cursor_motion_treats_an_emoji_as_one_character(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.simulate_input("a\u{1F642}b");
    press_key(cx, "left");
    press_key(cx, "left");
    cx.simulate_input("z");

    assert_eq!(
        text_of(&input, cx),
        "az\u{1F642}b",
        "the caret crossed the emoji in one step"
    );
}

#[gpui::test]
fn shift_selection_selects_whole_graphemes(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "a\u{1F642}b");

    press_key(cx, "shift-left");
    assert_eq!(
        selection_of(&input, cx),
        5..6,
        "one shift-left selects \"b\""
    );
    press_key(cx, "shift-left");
    assert_eq!(
        selection_of(&input, cx),
        1..6,
        "the second shift-left selects the emoji as one character"
    );
}

#[gpui::test]
fn a_read_only_input_ignores_edits_but_still_moves_and_selects(cx: &mut TestAppContext) {
    let (input, cx) = open_input(cx, "locked value", |window, cx| {
        let mut input = TextInput::new(window, cx);
        input.set_read_only(true, cx);
        input
    });
    let mut events = cx.events(&input);

    cx.simulate_input("typed");
    press_key(cx, "backspace");
    press_key(cx, select_all_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "locked value",
        "a read-only input never changes its text"
    );
    assert_eq!(
        drain(&mut events),
        Vec::new(),
        "a read-only input emits no Changed"
    );
    assert_eq!(
        selection_of(&input, cx),
        0..12,
        "select-all still selects in a read-only input"
    );
}

#[gpui::test]
fn the_placeholder_shows_only_while_the_input_is_empty(cx: &mut TestAppContext) {
    let (_input, cx) = open_input(cx, "", |window, cx| {
        TextInput::new(window, cx).with_placeholder("Search collections")
    });

    assert!(
        cx.debug_bounds(PLACEHOLDER_SELECTOR).is_some(),
        "the empty input shows its placeholder"
    );

    cx.simulate_input("a");
    draw_window(cx);

    assert!(
        cx.debug_bounds(PLACEHOLDER_SELECTOR).is_none(),
        "typing takes the placeholder away"
    );
}
