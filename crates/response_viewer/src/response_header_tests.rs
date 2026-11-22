//! The response region is headed, and the facts under the heading read the way the
//! blueprint's Response header shows them (`06_visuals/workbench_blueprint.svg`,
//! S05/R05): a "Response" title, the status as a pill carrying the code and its reason,
//! and the time and size as labelled meta beside the pill — not bare numbers pinned to
//! the far right edge (#223). Every debug key here derives from the rendered text, so
//! what the test finds on screen is exactly what the user reads.

use std::time::Duration;

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};
use http_engine::{Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{
    RESPONSE_HEADER, response_header_selector, status_meta_selector, status_pill_selector,
};
use crate::{ResponseView, STATUS_LINE_SELECTOR};

#[gpui::test]
fn response_header_shows_title_status_pill_and_labelled_meta(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(vec![0; 244]));
    response.timings.total = Some(Duration::from_millis(184));
    view.update(cx, |view, cx| view.set_response(response, cx));
    draw_window(cx);

    assert!(
        shown(cx, response_header_selector(RESPONSE_HEADER)),
        "the response region is headed \"{RESPONSE_HEADER}\""
    );
    let pill_selector = status_pill_selector("200 OK");
    assert!(
        shown(cx, pill_selector.clone()),
        "the status draws as a pill with the code and its reason"
    );
    let time_selector = status_meta_selector("Time", "184 ms");
    let size_selector = status_meta_selector("Size", "244 B");
    assert!(
        shown(cx, time_selector.clone()),
        "the total time is labelled meta"
    );
    assert!(
        shown(cx, size_selector.clone()),
        "the body size is labelled meta"
    );

    let pill = bounds(cx, pill_selector);
    let time = bounds(cx, time_selector);
    let size = bounds(cx, size_selector);
    let row = bounds(cx, STATUS_LINE_SELECTOR.to_owned());
    assert!(
        pill.right() <= time.left() && time.right() <= size.left(),
        "pill, time and size read in that order: pill {pill:?}, time {time:?}, size {size:?}"
    );
    assert!(
        size.right() - pill.right() < row.right() - size.right(),
        "the labelled meta sits next to the status pill, not against the far right edge: \
         pill {pill:?}, size {size:?}, row {row:?}"
    );
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

/// Where the element under this debug key draws.
fn bounds(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    cx.debug_bounds(selector.leak())
        .expect("the element is on screen")
}
