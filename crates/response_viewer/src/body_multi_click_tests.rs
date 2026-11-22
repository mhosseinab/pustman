//! Selecting body text the way an editor does: a double click takes a word, a triple
//! click a line, a drag after either grows by that unit, and a drag that leaves the rows
//! keeps selecting — to the end below the last line, and scrolling past the pane's edge.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    ElementId, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, TestAppContext, VisualTestContext, px,
};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{BodyMode, BodyView};
use crate::child_id;

const RAW_BODY: &str = "alpha\nbravo charlie\ndelta";

fn text_response(body: &str) -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.as_bytes().to_vec()));
    response.headers = vec![Header::new("content-type", "text/plain")];
    response
}

/// The pane under test, focused, showing `body` in Raw.
fn raw_body_view<'a>(
    body: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<BodyView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(text_response(body)), cx);
        view.set_mode(BodyMode::Raw, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let handle = view.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    draw_window(cx);
    (view, cx)
}

fn line_bounds(
    view: &Entity<BodyView>,
    index: usize,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<Pixels> {
    let base = ElementId::from(("body-view", view.entity_id()));
    let selector = child_id(&base, format!("line-{index}")).to_string();
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("the line is painted")
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

fn press(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    draw_window(cx);
}

fn drag_to(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseMoveEvent {
        position: at,
        modifiers: Modifiers::none(),
        pressed_button: Some(MouseButton::Left),
    });
    draw_window(cx);
}

fn selected(view: &Entity<BodyView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.selected_text())
}

#[gpui::test]
fn test_a_double_click_selects_the_word_under_the_pointer(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(RAW_BODY, cx);
    let at = line_bounds(&view, 1, cx);
    // Well inside "bravo", the first word of "bravo charlie".
    let at = Point::new(at.left() + px(20.), at.center().y);
    click(at, 1, cx);
    click(at, 2, cx);

    assert_eq!(selected(&view, cx).as_deref(), Some("bravo"));
}

#[gpui::test]
fn test_a_triple_click_selects_the_whole_line_with_its_line_break(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(RAW_BODY, cx);
    let row = line_bounds(&view, 1, cx);
    let at = Point::new(row.left() + px(20.), row.center().y);
    click(at, 1, cx);
    click(at, 2, cx);
    click(at, 3, cx);

    assert_eq!(selected(&view, cx).as_deref(), Some("bravo charlie\n"));
}

#[gpui::test]
fn test_a_quadruple_click_selects_the_whole_body(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(RAW_BODY, cx);
    let row = line_bounds(&view, 0, cx);
    let at = Point::new(row.left() + px(20.), row.center().y);
    for count in 1..=4 {
        click(at, count, cx);
    }

    assert_eq!(selected(&view, cx).as_deref(), Some(RAW_BODY));
}

#[gpui::test]
fn test_a_drag_after_a_triple_click_grows_by_whole_lines(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(RAW_BODY, cx);
    let first = line_bounds(&view, 0, cx);
    let at = Point::new(first.left() + px(20.), first.center().y);
    click(at, 1, cx);
    click(at, 2, cx);
    press(at, 3, cx);
    let second = line_bounds(&view, 1, cx);
    drag_to(Point::new(second.left() + px(20.), second.center().y), cx);

    assert_eq!(
        selected(&view, cx).as_deref(),
        Some("alpha\nbravo charlie\n")
    );
}

#[gpui::test]
fn test_dragging_below_the_last_line_selects_to_the_end_of_the_body(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(RAW_BODY, cx);
    let first = line_bounds(&view, 0, cx);
    press(Point::new(first.left() + px(1.), first.center().y), 1, cx);
    let last = line_bounds(&view, 2, cx);
    drag_to(
        Point::new(first.left() + px(1.), last.bottom() + px(40.)),
        cx,
    );

    assert_eq!(selected(&view, cx).as_deref(), Some(RAW_BODY));
}

/// A body far taller than the window.
fn long_body() -> String {
    (0..500)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How far down the rows are scrolled.
fn scroll_top(view: &Entity<BodyView>, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        -view.list_scroll_handle.0.borrow().base_handle.offset().y
    })
}

#[gpui::test]
fn test_dragging_past_the_bottom_edge_scrolls_and_keeps_scrolling_while_held(
    cx: &mut TestAppContext,
) {
    let body = long_body();
    let (view, cx) = raw_body_view(&body, cx);
    let first = line_bounds(&view, 0, cx);
    press(Point::new(first.left() + px(1.), first.center().y), 1, cx);
    let below = cx.update(|window, _| window.viewport_size().height) + px(30.);
    drag_to(Point::new(first.left() + px(1.), below), cx);

    let top_after_move = scroll_top(&view, cx);
    assert!(
        top_after_move > px(0.),
        "a drag past the bottom edge scrolls the lines"
    );
    let head_after_move = view.read_with(cx, |view, _| view.selection_head_line());

    cx.executor().advance_clock(Duration::from_millis(500));
    draw_window(cx);
    let top_after_hold = scroll_top(&view, cx);
    assert!(
        top_after_hold > top_after_move,
        "holding the pointer past the edge keeps scrolling"
    );
    let head_after_hold = view.read_with(cx, |view, _| view.selection_head_line());
    assert!(
        head_after_hold > head_after_move,
        "the selection follows the scroll"
    );

    cx.simulate_event(MouseUpEvent {
        position: Point::new(first.left(), below),
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
    });
    draw_window(cx);
    let top_after_release = scroll_top(&view, cx);
    cx.executor().advance_clock(Duration::from_millis(500));
    draw_window(cx);
    let top_later = scroll_top(&view, cx);
    assert_eq!(top_later, top_after_release, "releasing stops the scroll");
}
