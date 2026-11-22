//! Selecting and copying body text by hand: a drag over the Pretty or Raw lines selects
//! them, the copy keystroke puts the selection on the clipboard, and select-all takes the
//! whole body.

use std::sync::Arc;

use gpui::{
    ElementId, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, px,
};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{BodyMode, BodyView};
use crate::child_id;

const RAW_BODY: &str = "alpha\nbravo\ncharlie";

fn text_response(body: &str) -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.as_bytes().to_vec()));
    response.headers = vec![Header::new("content-type", "text/plain")];
    response
}

/// The pane under test, focused, showing [`RAW_BODY`] in Raw.
fn raw_body_view(cx: &mut TestAppContext) -> (Entity<BodyView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(text_response(RAW_BODY)), cx);
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
    cx.debug_bounds(leak(selector))
        .expect("the line is painted")
}

/// Left of the first character of row `index`.
fn row_start(view: &Entity<BodyView>, index: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let row = line_bounds(view, index, cx);
    Point::new(row.left() + px(1.), row.center().y)
}

/// Past the last character of row `index`.
fn row_end(view: &Entity<BodyView>, index: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let row = line_bounds(view, index, cx);
    Point::new(row.right() - px(1.), row.center().y)
}

fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    draw_window(cx);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    draw_window(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    draw_window(cx);
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

fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

#[gpui::test]
fn test_dragging_across_two_lines_and_copying_puts_those_lines_on_the_clipboard(
    cx: &mut TestAppContext,
) {
    let (view, cx) = raw_body_view(cx);
    let from = row_start(&view, 0, cx);
    let to = row_end(&view, 1, cx);
    drag(from, to, cx);

    cx.simulate_keystrokes(copy_keystroke());
    cx.run_until_parked();

    assert_eq!(clipboard_text(cx).as_deref(), Some("alpha\nbravo"));
}

#[gpui::test]
fn test_a_drag_within_one_line_copies_only_the_dragged_characters(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(cx);
    let from = row_start(&view, 2, cx);
    let end = row_end(&view, 2, cx);
    drag(from, end, cx);
    let selected = view.read_with(cx, |view, _| view.selected_text());
    assert_eq!(selected.as_deref(), Some("charlie"));
}

#[gpui::test]
fn test_select_all_then_copy_puts_the_whole_shown_body_on_the_clipboard(cx: &mut TestAppContext) {
    let (_view, cx) = raw_body_view(cx);
    cx.simulate_keystrokes(select_all_keystroke());
    cx.simulate_keystrokes(copy_keystroke());
    cx.run_until_parked();

    assert_eq!(clipboard_text(cx).as_deref(), Some(RAW_BODY));
}

#[gpui::test]
fn test_a_new_body_drops_the_old_selection(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(cx);
    cx.simulate_keystrokes(select_all_keystroke());
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(text_response("other")), cx);
    });
    cx.run_until_parked();

    let selected = view.read_with(cx, |view, _| view.selected_text());
    assert_eq!(
        selected, None,
        "a selection never outlives the lines it was made on"
    );
}

#[gpui::test]
fn test_a_click_without_a_drag_clears_the_selection(cx: &mut TestAppContext) {
    let (view, cx) = raw_body_view(cx);
    cx.simulate_keystrokes(select_all_keystroke());
    let at = row_start(&view, 1, cx);
    drag(at, at, cx);

    let selected = view.read_with(cx, |view, _| view.selected_text());
    assert_eq!(selected, None);
}

/// The row selector is built at run time, and `debug_bounds` keys on `'static` strings.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}
