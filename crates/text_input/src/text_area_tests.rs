//! The [`TextArea`] as a pointer sees it: a real window, a real mouse event dispatched
//! through the element's hitboxes, and exact assertions on where the caret lands. The
//! widget fills its box while its text does not, so a hit below the text is a hit on
//! the widget — and must still land at the text's end.

use std::ops::Range;

use gpui::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext, point, px};
use ui::test_support::{draw_window, init_test};

use super::TextArea;
use crate::init;

/// A window with one text area holding `text`, drawn and focused, ready for input.
fn focused_text_area<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextArea>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (area, cx) = cx.add_window_view(|window, cx| {
        let mut area = TextArea::new(window, cx);
        area.set_text(text, cx);
        area
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| area.read_with(cx, |area, cx| area.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (area, cx)
}

/// The text area's selected byte range — an empty range is the caret.
fn selection_of(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> Range<usize> {
    cx.read(|cx| area.read_with(cx, |area, _| area.selected_range()))
}

#[gpui::test]
fn clicking_below_the_text_puts_the_caret_at_the_end(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx, "hello");

    cx.simulate_click(point(px(20.), px(300.)), Modifiers::none());

    assert_eq!(
        selection_of(&area, cx),
        5..5,
        "a click below the single row puts the caret at the text end"
    );
}
