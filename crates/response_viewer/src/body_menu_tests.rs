//! The body's right-click copy menu: on a Pretty JSON line it copies the clicked value,
//! its property path and the string in DevTools' forms; anywhere it copies the selection.

use std::sync::Arc;

use gpui::{
    ElementId, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, px,
};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{BodyMode, BodyView};
use crate::child_id;

/// Pretty-printed, line 3 is `"name": "Ann"` — the last token on it is the string value.
const JSON_BODY: &str = r#"{"data":[{"name":"Ann"}]}"#;
const NAME_LINE: usize = 3;

fn response(body: &str, content_type: &str) -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.as_bytes().to_vec()));
    response.headers = vec![Header::new("content-type", content_type)];
    response
}

fn body_view(
    response: Response,
    mode: BodyMode,
    cx: &mut TestAppContext,
) -> (Entity<BodyView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response), cx);
        view.set_mode(mode, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let handle = view.read(cx).focus_handle.clone();
        window.focus(&handle, cx);
    });
    draw_window(cx);
    (view, cx)
}

/// Past the last character of row `index`.
fn row_end(view: &Entity<BodyView>, index: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let base = ElementId::from(("body-view", view.entity_id()));
    let selector = child_id(&base, format!("line-{index}")).to_string();
    let row = cx
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("the line is painted");
    Point::new(row.right() - px(1.), row.center().y)
}

fn right_click(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    draw_window(cx);
}

fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

#[gpui::test]
fn test_right_clicking_a_json_value_offers_its_property_path(cx: &mut TestAppContext) {
    let (view, cx) = body_view(
        response(JSON_BODY, "application/json"),
        BodyMode::Pretty,
        cx,
    );
    let at = row_end(&view, NAME_LINE, cx);
    right_click(at, cx);

    // Copy value, then Copy property path.
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();

    assert_eq!(clipboard_text(cx).as_deref(), Some("data[0].name"));
}

#[gpui::test]
fn test_right_clicking_a_json_string_copies_it_as_a_javascript_literal(cx: &mut TestAppContext) {
    let (view, cx) = body_view(
        response(JSON_BODY, "application/json"),
        BodyMode::Pretty,
        cx,
    );
    let at = row_end(&view, NAME_LINE, cx);
    right_click(at, cx);

    // Copy value, Copy property path, Copy string contents, Copy string as JavaScript literal.
    cx.simulate_keystrokes("down down down enter");
    cx.run_until_parked();

    assert_eq!(clipboard_text(cx).as_deref(), Some("'Ann'"));
}

#[gpui::test]
fn test_right_clicking_a_selection_offers_copy_first(cx: &mut TestAppContext) {
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    let (view, cx) = body_view(response("alpha\nbravo", "text/plain"), BodyMode::Raw, cx);
    cx.simulate_keystrokes(select_all);
    let at = row_end(&view, 0, cx);
    right_click(at, cx);

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(clipboard_text(cx).as_deref(), Some("alpha\nbravo"));
}
