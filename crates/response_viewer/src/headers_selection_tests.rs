//! Header names and values are text the user copies out: a double click takes a word, a
//! triple click the row as `name: value`, and select-all then copy takes the table.

use gpui::{
    ElementId, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, px,
};
use http_engine::Header;
use ui::test_support::{draw_window, init_test};

use super::HeadersView;
use crate::child_id;

fn headers_view(cx: &mut TestAppContext) -> (Entity<HeadersView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| HeadersView::new(cx));
    let headers = vec![
        Header::new("content-type", "text/plain"),
        Header::new("x-id", "abc def"),
    ];
    view.update(cx, |view, cx| view.set_headers(&headers, cx));
    draw_window(cx);
    (view, cx)
}

fn cell_bounds(
    view: &Entity<HeadersView>,
    row_key: &str,
    column: &str,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<Pixels> {
    let base = ElementId::from(("headers-view", view.entity_id()));
    let row = child_id(&base, row_key.to_owned());
    let selector = child_id(&row, column.to_owned()).to_string();
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("the cell is painted")
}

fn click(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    draw_window(cx);
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
    });
    draw_window(cx);
}

fn clicks(at: Point<Pixels>, count: usize, cx: &mut VisualTestContext) {
    for click_count in 1..=count {
        click(at, click_count, cx);
    }
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn copy_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    }
}

fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[gpui::test]
fn test_a_double_click_on_a_header_value_copies_the_word_under_the_pointer(
    cx: &mut TestAppContext,
) {
    let (view, cx) = headers_view(cx);
    let value = cell_bounds(&view, "x-id#0", "value", cx);
    clicks(Point::new(value.left() + px(2.), value.center().y), 2, cx);
    cx.simulate_keystrokes(copy_keystroke());

    assert_eq!(clipboard(cx).as_deref(), Some("abc"));
}

#[gpui::test]
fn test_a_triple_click_copies_the_header_row_as_name_colon_value(cx: &mut TestAppContext) {
    let (view, cx) = headers_view(cx);
    let name = cell_bounds(&view, "content-type#0", "name", cx);
    clicks(Point::new(name.left() + px(2.), name.center().y), 3, cx);
    cx.simulate_keystrokes(copy_keystroke());

    assert_eq!(clipboard(cx).as_deref(), Some("content-type: text/plain\n"));
}

#[gpui::test]
fn test_select_all_then_copy_takes_every_header(cx: &mut TestAppContext) {
    let (view, cx) = headers_view(cx);
    let name = cell_bounds(&view, "content-type#0", "name", cx);
    click(Point::new(name.left() + px(2.), name.center().y), 1, cx);
    cx.simulate_keystrokes(select_all_keystroke());
    cx.simulate_keystrokes(copy_keystroke());

    assert_eq!(
        clipboard(cx).as_deref(),
        Some("content-type: text/plain\nx-id: abc def")
    );
}
