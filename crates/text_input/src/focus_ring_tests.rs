//! A focused text field says so: a `border_focused` ring over the field, shown however
//! focus arrived (a click focuses a field too, so this is not `focus_visible`), and gone
//! when focus leaves (#212 — the focused URL field looked like every other field).

use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::{FOCUS_RING_SELECTOR, TextArea, TextInput, init};

fn focus<T: Focusable>(view: &Entity<T>, cx: &mut VisualTestContext) {
    let focus_handle = cx.read(|cx| view.read_with(cx, |view, cx| view.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
}

#[gpui::test]
fn test_a_text_input_shows_a_focus_ring_only_while_focused(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(TextInput::new);
    draw_window(cx);
    assert!(
        cx.debug_bounds(FOCUS_RING_SELECTOR).is_none(),
        "an unfocused field keeps its rest border only"
    );

    focus(&input, cx);
    let ring = cx
        .debug_bounds(FOCUS_RING_SELECTOR)
        .expect("the focused field draws its ring");
    let field = cx
        .debug_bounds(crate::input::TEXT_INPUT_SELECTOR)
        .expect("the field is laid out");
    assert_eq!(ring, field, "the ring traces the whole field");

    cx.update(|window, cx| window.blur(cx));
    draw_window(cx);
    assert!(
        cx.debug_bounds(FOCUS_RING_SELECTOR).is_none(),
        "the ring goes when focus leaves"
    );
}

#[gpui::test]
fn test_a_text_area_shows_a_focus_ring_while_focused(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (area, cx) = cx.add_window_view(TextArea::new);
    draw_window(cx);
    assert!(cx.debug_bounds(FOCUS_RING_SELECTOR).is_none());

    focus(&area, cx);
    assert!(
        cx.debug_bounds(FOCUS_RING_SELECTOR).is_some(),
        "the focused text area draws its ring"
    );
}
