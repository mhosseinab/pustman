//! A body line's hover tooltip is a truncation tooltip and nothing else (#268).
//!
//! Every line used to carry its plain text in a hover tooltip whatever its length, so
//! hovering a fully visible Pretty line covered its token-coloured text with an
//! uncoloured copy (`seg2.png`). Only a line the pane actually truncates has anything
//! the hover can add: the tail the ellipsis swallowed.

use std::sync::Arc;
use std::time::Duration;

use gpui::{ElementId, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::components::tooltip::TOOLTIP_SELECTOR;
use ui::test_support::{draw_window, init_test};

use super::BodyView;
use crate::child_id;

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// How far into a row the tests rest the pointer: past the row's padding and well within
/// the first characters of any line they hover.
const HOVER_INTO_ROW_PX: f32 = 24.;

/// A Pretty JSON body: one short line and one no window can show whole.
fn pretty_body() -> Response {
    let body = format!("{{\"ok\": true, \"blob\": \"{}\"}}", "x".repeat(2000));
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.into_bytes()));
    response.headers = vec![Header::new("content-type", "application/json")];
    response
}

/// The pane under test: Pretty mode, the JSON body above formatted and on screen.
fn body_view(cx: &mut TestAppContext) -> (Entity<BodyView>, &mut VisualTestContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| view.set_body(Arc::new(pretty_body()), cx));
    cx.run_until_parked();
    draw_window(cx);
    (view, cx)
}

/// The bounds of the line row `index` draws in (`VirtualList` gives every row its item
/// key as its debug selector).
fn line_bounds(
    view: &Entity<BodyView>,
    index: usize,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    let base = ElementId::from(("body-view", view.entity_id()));
    let selector = child_id(&base, format!("line-{index}")).to_string();
    cx.debug_bounds(leak(selector))
        .expect("the line is painted")
}

/// Rest the pointer on the line row until a tooltip would show; report whether one did.
fn tooltip_after_hovering_line(
    view: &Entity<BodyView>,
    index: usize,
    cx: &mut VisualTestContext,
) -> bool {
    draw_window(cx);
    let row = line_bounds(view, index, cx);
    let hover = gpui::Point::new(row.left() + gpui::px(HOVER_INTO_ROW_PX), row.center().y);
    cx.simulate_mouse_move(hover, None::<MouseButton>, Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    cx.debug_bounds(TOOLTIP_SELECTOR).is_some()
}

/// The index of the drawn line holding `needle`.
fn line_with(view: &Entity<BodyView>, needle: &str, cx: &VisualTestContext) -> usize {
    view.read_with(cx, |view, _| {
        view.shown_lines()
            .iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("the body draws a line holding {needle:?}"))
    })
}

#[gpui::test]
fn test_a_fully_visible_line_is_its_own_tooltip(cx: &mut TestAppContext) {
    let (view, cx) = body_view(cx);
    let short = line_with(&view, "\"ok\"", cx);
    assert!(
        !tooltip_after_hovering_line(&view, short, cx),
        "a line drawn whole repeats itself in no balloon — the coloured line below is \
         its own tooltip"
    );
}

#[gpui::test]
fn test_a_truncated_line_carries_its_full_text_in_a_tooltip(cx: &mut TestAppContext) {
    let (view, cx) = body_view(cx);
    let long = line_with(&view, "xxxx", cx);
    assert!(
        tooltip_after_hovering_line(&view, long, cx),
        "a line the pane truncates shows the whole line on hover"
    );
}

/// The row selector is built at run time, and `debug_bounds` keys on `'static` strings —
/// the test-only leak the `timeline` tests use for the same shape of selector.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}
