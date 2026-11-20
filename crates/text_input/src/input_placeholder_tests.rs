//! The placeholder is drawn exactly once, where typed text would start (S25 smoke: the
//! palette's "Type a command" showed twice, one copy offset from the other).

use gpui::{TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::{PLACEHOLDER_SELECTOR, TextInput};
use crate::init;

fn empty_input_with_placeholder(
    cx: &mut TestAppContext,
) -> (gpui::Entity<TextInput>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(|window, cx| {
        TextInput::new(window, cx).with_placeholder("Type a command")
    });
    draw_window(cx);
    (input, cx)
}

#[gpui::test]
fn test_empty_input_draws_its_placeholder_once(cx: &mut TestAppContext) {
    let (input, cx) = empty_input_with_placeholder(cx);

    let shaped = cx.read(|cx| {
        input.read_with(cx, |input, _| {
            input
                .last_layout
                .borrow()
                .as_ref()
                .map(|line| line.text.to_string())
        })
    });
    assert_eq!(
        shaped.as_deref(),
        Some(""),
        "the text line of an empty input shapes nothing; the placeholder label is the only copy"
    );
}

#[gpui::test]
fn test_placeholder_starts_where_typed_text_starts(cx: &mut TestAppContext) {
    let (input, cx) = empty_input_with_placeholder(cx);

    let label = cx
        .debug_bounds(PLACEHOLDER_SELECTOR)
        .expect("the empty input shows its placeholder");
    let text_line = cx
        .read(|cx| input.read_with(cx, |input, _| input.last_bounds.get()))
        .expect("the text line was laid out");
    assert_eq!(
        label.origin, text_line.origin,
        "the placeholder sits exactly where the first typed character would"
    );
}
