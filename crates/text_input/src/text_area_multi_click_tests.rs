//! Multi-click selection in the text area: a triple click selects the logical line under
//! the pointer with its line break, as Zed's editor does, and a fourth click everything.

use gpui::{
    Entity, Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point, px,
};
use ui::test_support::{draw_window, init_test};

use super::TextArea;
use crate::init;

const BODY: &str = "{\n  \"name\": \"Ann\"\n}";

fn focused_text_area(cx: &mut TestAppContext) -> (Entity<TextArea>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (area, cx) = cx.add_window_view(|window, cx| {
        let mut area = TextArea::new(window, cx);
        area.set_text(BODY, cx);
        area
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| area.read_with(cx, |area, cx| area.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (area, cx)
}

/// A point whose hit lands on byte `offset`, found by walking the painted area.
fn point_at(area: &Entity<TextArea>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let bounds = cx
        .read(|cx| area.read(cx).painted_bounds())
        .expect("the text area painted");
    let line_height = cx.update(|window, _| window.line_height());
    let mut y = bounds.top();
    while y < bounds.bottom() {
        let mut x = bounds.left();
        while x < bounds.right() {
            let at = point(x, y);
            if cx.read(|cx| area.read(cx).index_for_mouse_position(at, line_height)) == offset {
                return at;
            }
            x += px(0.5);
        }
        y += px(1.);
    }
    panic!("no point of the area lands on byte {offset}");
}

fn click(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
    });
}

fn selected(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> String {
    let range = cx.read(|cx| area.read(cx).selected_range());
    BODY.get(range).unwrap_or_default().to_string()
}

#[gpui::test]
fn test_a_double_click_in_the_text_area_selects_the_word(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx);
    let at = point_at(&area, BODY.find("name").unwrap_or_default() + 1, cx);
    click(at, 1, cx);
    click(at, 2, cx);
    assert_eq!(selected(&area, cx), "name");
}

#[gpui::test]
fn test_a_triple_click_selects_the_line_with_its_line_break(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx);
    let at = point_at(&area, BODY.find("name").unwrap_or_default() + 1, cx);
    click(at, 1, cx);
    click(at, 2, cx);
    click(at, 3, cx);
    assert_eq!(selected(&area, cx), "  \"name\": \"Ann\"\n");
}

#[gpui::test]
fn test_a_fourth_click_selects_everything(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx);
    let at = point_at(&area, 1, cx);
    for count in 1..=4 {
        click(at, count, cx);
    }
    assert_eq!(selected(&area, cx), BODY);
}
