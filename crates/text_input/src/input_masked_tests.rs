//! Masked mode (#178): a masked field shows one [`MASK_CHARACTER`] per character of
//! its text — to the painted line and to the platform alike — while the buffer keeps
//! the value and every edit keeps working. The assertions read the shaped line
//! (`last_layout`, exactly what `paint` hands the screen) and the `EntityInputHandler`
//! surface the platform goes through.

use gpui::{
    Context, Entity, EntityInputHandler, Focusable, TestAppContext, VisualTestContext, Window,
    point, px,
};
use ui::test_support::{draw_window, init_test, press_key};

use super::{MASK_CHARACTER, TextInput};
use crate::init;

/// An obviously fake fixture secret, so secret scanners and reviewers do not trip on it.
const FAKE_SECRET: &str = "hunter2-REDACTED-example";

/// A masked field holding `text`, focused and drawn.
fn masked_input<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextInput>, &'a mut VisualTestContext) {
    open_input(cx, text, |window, cx| {
        TextInput::new(window, cx).with_masked()
    })
}

/// The same, with `build` shaping the field first (masked or not).
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
    let focus_handle = cx.read(|cx| input.read_with(cx, |input, cx| input.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (input, cx)
}

/// What the field's text line shapes — the painted text.
fn shaped_text(input: &Entity<TextInput>, cx: &VisualTestContext) -> String {
    cx.read(|cx| {
        input.read_with(cx, |input, _| {
            input
                .last_layout
                .borrow()
                .as_ref()
                .map(|line| line.text.to_string())
                .unwrap_or_default()
        })
    })
}

#[gpui::test]
fn test_a_masked_field_paints_masks_and_keeps_its_value(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, FAKE_SECRET);

    let shaped = shaped_text(&input, cx);
    assert_eq!(
        shaped,
        MASK_CHARACTER
            .to_string()
            .repeat(FAKE_SECRET.chars().count()),
        "the painted line is one mask per character of the value"
    );
    assert!(
        !shaped.contains(FAKE_SECRET),
        "the value never reaches the painted line"
    );
    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input.text())),
        FAKE_SECRET,
        "the buffer keeps the value: the field still edits and reports it"
    );
}

#[gpui::test]
fn test_an_unmasked_field_paints_its_value_unchanged(cx: &mut TestAppContext) {
    let (input, cx) = open_input(cx, FAKE_SECRET, TextInput::new);

    assert_eq!(
        shaped_text(&input, cx),
        FAKE_SECRET,
        "an unmasked field echoes what it holds, as ever"
    );
}

#[gpui::test]
fn test_masks_count_characters_not_bytes(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "a\u{1F642}b");

    assert_eq!(
        shaped_text(&input, cx),
        MASK_CHARACTER.to_string().repeat(3),
        "one mask per character: the emoji is one mask, not its four bytes"
    );
}

#[gpui::test]
fn test_typing_grows_the_masks_by_one_per_character(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "");

    cx.simulate_input("ab\u{1F642}");
    draw_window(cx);

    assert_eq!(
        shaped_text(&input, cx),
        MASK_CHARACTER.to_string().repeat(3),
        "every typed character adds exactly one mask"
    );
    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input.text())),
        "ab\u{1F642}",
        "what was typed lands in the buffer"
    );
}

#[gpui::test]
fn test_a_click_inside_the_masks_puts_the_caret_in_the_matching_place(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "abc");

    // The masks are uniform, so a quarter of the line width lands inside the second
    // mask — the caret belongs just after the first character.
    let in_second_mask = cx.read(|cx| {
        input.read_with(cx, |input, _| {
            let bounds = input.last_bounds.get().expect("the field was drawn");
            let line = input.last_layout.borrow();
            let line = line.as_ref().expect("the field was drawn");
            point(
                bounds.left() + line.width * 0.25,
                bounds.top() + bounds.size.height / 2.,
            )
        })
    });
    let past_the_end = cx.read(|cx| {
        input.read_with(cx, |input, _| {
            let bounds = input.last_bounds.get().expect("the field was drawn");
            point(
                bounds.right() + px(100.),
                bounds.top() + bounds.size.height / 2.,
            )
        })
    });

    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input
            .index_for_mouse_position(in_second_mask))),
        1,
        "a click inside the second mask puts the caret after \"a\", not after a mask byte"
    );
    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input.index_for_mouse_position(past_the_end))),
        3,
        "a click past the masks puts the caret at the end of the value"
    );
}

#[gpui::test]
fn test_the_platform_reads_masks_from_a_masked_field(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "abc");

    let (surrounding, length) = cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            (
                input.text_for_range(0..3, &mut None, window, cx),
                input.text_length_utf16(window, cx),
            )
        })
    });

    assert_eq!(
        surrounding.as_deref(),
        Some(MASK_CHARACTER.to_string().repeat(3).as_str()),
        "the platform's surrounding-text query reads masks, never the value"
    );
    assert_eq!(
        length,
        Some(3),
        "the platform counts masks, one per character"
    );
}

#[gpui::test]
fn test_an_edit_through_the_platform_reaches_the_real_text(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "abc");

    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(Some(0..1), "X", window, cx)
        })
    });
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input.text())),
        "Xbc",
        "the platform's masked view is display-deep: the edit lands in the value"
    );
    assert_eq!(
        shaped_text(&input, cx),
        MASK_CHARACTER.to_string().repeat(3),
        "and the field keeps showing masks"
    );
}

#[gpui::test]
fn test_ime_bounds_line_up_with_the_masks_not_the_text(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "abc");

    let (line_width, bounds) = cx.read(|cx| {
        input.read_with(cx, |input, _| {
            let bounds = input.last_bounds.get().expect("the field was drawn");
            let line = input.last_layout.borrow();
            let line = line.as_ref().expect("the field was drawn");
            (line.width, bounds)
        })
    });
    let caret_at_end = cx
        .update(|window, cx| {
            input.update(cx, |input, cx| {
                input.bounds_for_range(3..3, bounds, window, cx)
            })
        })
        .expect("the field was shaped");

    assert_eq!(
        caret_at_end.left(),
        bounds.left() + line_width,
        "the caret the IME asks about sits past the last mask, not past the last byte"
    );
}

#[gpui::test]
fn test_shift_selection_covers_the_masks_of_the_whole_value(cx: &mut TestAppContext) {
    let (input, cx) = masked_input(cx, "abc");

    press_key(cx, "shift-home");

    assert_eq!(
        cx.read(|cx| input.read_with(cx, |input, _| input.selected_range())),
        0..3,
        "selection stays in value offsets while the field shows masks"
    );
}
