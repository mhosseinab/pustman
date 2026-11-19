//! The undo model and the clipboard actions, driven the way a user drives them: a real
//! window with focus, keystrokes through the keys [`crate::init`] binds, and exact
//! assertions on text, selection and the platform clipboard. The setup helpers mirror
//! `input_tests`'s (that module is read-only for this step), so a field that stops
//! recording, coalescing or pasting fails here — not only one that panics.

use std::ops::Range;

use gpui::{
    ClipboardItem, Context, Entity, EntityInputHandler, Focusable, TestAppContext,
    VisualTestContext, Window,
};
use ui::test_support::{draw_window, init_test, press_key};

use super::{MAX_TRANSACTIONS, Record, Selection, Transaction, UndoStack};
use crate::init;
use crate::input::TextInput;

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

/// The field's text, read through the entity.
fn text_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    cx.read(|cx| input.read_with(cx, |input, _| input.text()))
}

/// The field's selected byte range — an empty range is the caret.
fn selection_of(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> Range<usize> {
    cx.read(|cx| input.read_with(cx, |input, _| input.selected_range()))
}

/// Put `text` on the platform clipboard the widget reads in `Paste`.
fn set_clipboard(cx: &mut VisualTestContext, text: &str) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
}

/// The text on the platform clipboard, whatever put it there.
fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

fn copy_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    }
}

fn cut_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-x"
    } else {
        "ctrl-x"
    }
}

fn paste_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

fn undo_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    }
}

fn redo_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-shift-z"
    } else {
        "ctrl-shift-z"
    }
}

/// A one-grapheme insertion at `offset`, as the undo model records it.
fn insertion(offset: usize) -> Transaction {
    Transaction {
        replaced_range: offset..offset,
        old_text: String::new(),
        new_text: "x".to_string(),
        selection_before: Selection::caret(offset),
        selection_after: Selection::caret(offset + 1),
    }
}

#[gpui::test]
fn undo_redo_restores_text(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "hello ");
    set_clipboard(cx, "world");

    press_key(cx, paste_keystroke());
    assert_eq!(
        text_of(&input, cx),
        "hello world",
        "paste lands at the caret"
    );

    press_key(cx, undo_keystroke());
    assert_eq!(text_of(&input, cx), "hello ", "undo takes the paste back");

    press_key(cx, redo_keystroke());
    assert_eq!(
        text_of(&input, cx),
        "hello world",
        "redo puts the paste back"
    );
}

#[gpui::test]
fn paste_replaces_selection(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "hello brave world");
    set_clipboard(cx, "shy");

    press_key(cx, select_all_keystroke());
    press_key(cx, paste_keystroke());

    assert_eq!(text_of(&input, cx), "shy", "paste replaces the selection");
    assert_eq!(
        selection_of(&input, cx),
        3..3,
        "the caret sits after the pasted text"
    );
}

#[gpui::test]
fn typing_coalesces_into_one_undo_step(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.simulate_input("hello");
    press_key(cx, undo_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "",
        "one undo takes back the whole typing run, not one character"
    );
}

#[gpui::test]
fn cursor_motion_breaks_the_typing_run(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.simulate_input("ab");
    press_key(cx, "left");
    cx.simulate_input("c");
    assert_eq!(
        text_of(&input, cx),
        "acb",
        "typing lands at the moved caret"
    );

    press_key(cx, undo_keystroke());
    assert_eq!(
        text_of(&input, cx),
        "ab",
        "the typing after the cursor move undoes on its own"
    );
    press_key(cx, undo_keystroke());
    assert_eq!(
        text_of(&input, cx),
        "",
        "and the typing before the move undoes before that"
    );
}

#[gpui::test]
fn undo_restores_the_selection_not_just_the_text(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "hello");

    press_key(cx, "shift-left");
    press_key(cx, "shift-left");
    assert_eq!(selection_of(&input, cx), 3..5, "shift-left selects \"lo\"");
    cx.simulate_input("y");
    assert_eq!(
        text_of(&input, cx),
        "hely",
        "typing replaces the selected \"lo\" with \"y\""
    );

    press_key(cx, undo_keystroke());

    assert_eq!(text_of(&input, cx), "hello", "undo restores the text");
    assert_eq!(
        selection_of(&input, cx),
        3..5,
        "undo restores the selection, not just the text"
    );
}

#[gpui::test]
fn a_new_edit_clears_the_redo_stack(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.simulate_input("ab");
    press_key(cx, undo_keystroke());
    assert_eq!(text_of(&input, cx), "", "undo takes the typing back");

    cx.simulate_input("x");
    press_key(cx, redo_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "x",
        "the new edit dropped the undone one: redo must not resurrect \"ab\""
    );
}

#[gpui::test]
fn copy_then_paste_round_trips(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "h\u{e9}llo \u{1F44B}");

    press_key(cx, select_all_keystroke());
    press_key(cx, copy_keystroke());
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("h\u{e9}llo \u{1F44B}"),
        "copy fills the clipboard with the selection"
    );

    press_key(cx, "end");
    press_key(cx, paste_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "h\u{e9}llo \u{1F44B}h\u{e9}llo \u{1F44B}",
        "paste round-trips the copied multi-byte text"
    );
}

#[gpui::test]
fn copy_with_an_empty_selection_keeps_the_clipboard(cx: &mut TestAppContext) {
    let (_input, cx) = focused_input(cx, "value");
    set_clipboard(cx, "clipboard before");

    press_key(cx, copy_keystroke());

    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("clipboard before"),
        "copy with no selection must not clobber the clipboard"
    );
}

#[gpui::test]
fn cut_removes_the_selection_and_fills_the_clipboard(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "hello world");

    press_key(cx, select_all_keystroke());
    press_key(cx, cut_keystroke());

    assert_eq!(text_of(&input, cx), "", "cut removes the selection");
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("hello world"),
        "cut fills the clipboard with the selection"
    );

    press_key(cx, undo_keystroke());
    assert_eq!(text_of(&input, cx), "hello world", "cut is one undo step");
}

#[gpui::test]
fn a_read_only_input_rejects_cut_and_paste_but_still_copies(cx: &mut TestAppContext) {
    let (input, cx) = open_input(cx, "locked value", |window, cx| {
        let mut input = TextInput::new(window, cx);
        input.set_read_only(true, cx);
        input
    });
    set_clipboard(cx, "clipboard before");

    press_key(cx, select_all_keystroke());
    press_key(cx, cut_keystroke());
    press_key(cx, paste_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "locked value",
        "a read-only input never changes its text"
    );
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("clipboard before"),
        "a rejected cut must not clobber the clipboard"
    );

    press_key(cx, copy_keystroke());
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("locked value"),
        "copy still works in a read-only input"
    );
}

#[gpui::test]
fn pasted_newlines_become_spaces(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");
    set_clipboard(cx, "a\r\nb\nc\rd");

    press_key(cx, paste_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "a b c d",
        "every pasted line break becomes one space"
    );
}

#[gpui::test]
fn ime_composition_undoes_as_one_transaction(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            input.replace_and_mark_text_in_range(None, "nihao", None, window, cx);
            input.replace_text_in_range(None, "\u{4F60}\u{597D}", window, cx);
        });
    });
    assert_eq!(
        text_of(&input, cx),
        "\u{4F60}\u{597D}",
        "the composition commits its text"
    );

    press_key(cx, undo_keystroke());

    assert_eq!(
        text_of(&input, cx),
        "",
        "one undo takes back the whole composition, not one update"
    );
}

#[cfg(not(target_os = "macos"))]
#[gpui::test]
fn ctrl_y_redoes_on_non_mac(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx, "");

    cx.simulate_input("ab");
    press_key(cx, undo_keystroke());
    press_key(cx, "ctrl-y");

    assert_eq!(
        text_of(&input, cx),
        "ab",
        "ctrl-y is the Windows/Linux redo twin"
    );
}

#[gpui::test]
fn the_undo_stack_drops_the_oldest_transaction_at_the_bound(_cx: &mut TestAppContext) {
    let mut stack = UndoStack::new();
    for offset in 0..(MAX_TRANSACTIONS + 5) {
        stack.record(insertion(offset), Record::Atomic);
    }

    let mut undone = 0;
    while stack.undo().is_some() {
        undone += 1;
    }

    assert_eq!(
        undone, MAX_TRANSACTIONS,
        "the bound holds: only the newest transactions survive"
    );
}
