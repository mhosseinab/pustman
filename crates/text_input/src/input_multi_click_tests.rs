//! Multi-click selection in the single-line input, the way Zed's editor does it: a
//! double click selects the word under the pointer, a triple click the line (the whole
//! field), and a drag that follows a double click grows the selection word by word.

use std::ops::Range;

use gpui::{
    Entity, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, VisualTestContext, point, px,
};
use ui::test_support::{draw_window, init_test};

use super::TextInput;
use crate::init;

const URL: &str = "https://api.example.com/users?page=2";

fn focused_input(cx: &mut TestAppContext) -> (Entity<TextInput>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new(window, cx);
        input.set_text(URL, cx);
        input
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| input.read_with(cx, |input, cx| input.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (input, cx)
}

/// A point on the text whose hit lands on byte `offset`, found by walking the field.
fn point_at(input: &Entity<TextInput>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let bounds = cx
        .read(|cx| input.read(cx).last_bounds.get())
        .expect("the input's text is painted");
    let y = bounds.center().y;
    let mut x = bounds.left();
    while x < bounds.right() {
        let at = point(x, y);
        if cx.read(|cx| input.read(cx).index_for_mouse_position(at)) == offset {
            return at;
        }
        x += px(0.5);
    }
    panic!("no point of the field lands on byte {offset}");
}

fn press(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
}

fn release(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
    });
}

fn click(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    press(at, click_count, cx);
    release(at, click_count, cx);
}

fn selected(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    let range: Range<usize> = cx.read(|cx| input.read(cx).selected_range());
    URL.get(range).unwrap_or_default().to_string()
}

fn offset_of(needle: &str) -> usize {
    URL.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} is in the URL"))
}

#[gpui::test]
fn test_a_double_click_selects_the_word_under_the_pointer(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx);
    let at = point_at(&input, offset_of("example") + 2, cx);
    click(at, 1, cx);
    click(at, 2, cx);
    assert_eq!(selected(&input, cx), "example");
}

#[gpui::test]
fn test_a_triple_click_selects_the_whole_field(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx);
    let at = point_at(&input, offset_of("users") + 1, cx);
    click(at, 1, cx);
    click(at, 2, cx);
    click(at, 3, cx);
    assert_eq!(selected(&input, cx), URL);
}

#[gpui::test]
fn test_a_drag_after_a_double_click_grows_the_selection_by_whole_words(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(cx);
    let from = point_at(&input, offset_of("example") + 2, cx);
    let to = point_at(&input, offset_of("users") + 1, cx);
    click(from, 1, cx);
    press(from, 2, cx);
    cx.simulate_event(MouseMoveEvent {
        position: to,
        modifiers: Modifiers::none(),
        pressed_button: Some(MouseButton::Left),
    });
    release(to, 2, cx);
    assert_eq!(selected(&input, cx), "example.com/users");
}
