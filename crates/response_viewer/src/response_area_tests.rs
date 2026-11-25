//! The response area as the approved mockup draws it (S48u5): the header reads
//! `Response  [404 Not Found]  Time 12 ms · Size 9 B  …  Save response`, with the pill
//! in its HTTP class's status tone; the Headers and Cookies tabs count what the response
//! carries; the body toolbar says which lines are on screen; and the empty state says
//! how to send.

use std::time::Duration;

use gpui::{Bounds, ElementId, Entity, Pixels, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::{
    SAVE_RESPONSE_SELECTOR, status_class_color, status_meta_selector, status_pill_selector,
};
use crate::body_view::LINE_RANGE_SELECTOR;
use crate::{ResponseView, SEND_HINT_SELECTOR};

fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

fn viewer(cx: &mut TestAppContext) -> (Entity<ResponseView>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap binds: {problems:?}"
        );
    });
    cx.add_window_view(|_, cx| ResponseView::new(cx))
}

fn show(view: &Entity<ResponseView>, response: Response, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_response(response, cx));
    cx.run_until_parked();
    draw_window(cx);
    // The line readout needs the list's first layout; the second frame reads it.
    draw_window(cx);
}

fn tab_count_selector(index: usize) -> String {
    format!("{}-count", ElementId::from(("response-tab", index)))
}

#[gpui::test]
fn test_a_404_header_shows_a_client_error_pill_then_time_dot_size_and_save(
    cx: &mut TestAppContext,
) {
    let (view, cx) = viewer(cx);
    let mut response = Response::new(404, "Not Found", ResponseBody::Bytes(b"not found".to_vec()));
    response.timings.total = Some(Duration::from_millis(12));
    show(&view, response, cx);

    let pill = bounds(cx, status_pill_selector("404 Not Found")).expect("the pill renders");
    let time = bounds(cx, status_meta_selector("Time", "12 ms")).expect("the time renders");
    let size = bounds(cx, status_meta_selector("Size", "9 B")).expect("the size renders");
    let save = bounds(cx, SAVE_RESPONSE_SELECTOR.to_owned()).expect("Save response renders");
    assert!(
        pill.right() <= time.left() && time.right() < size.left(),
        "pill, time and size read in order: {pill:?} {time:?} {size:?}"
    );
    assert!(
        size.right() < save.left(),
        "Save response sits at the header's right end: size {size:?}, save {save:?}"
    );

    let (client_error, success, ok_tone) = cx.update(|_, cx| {
        (
            status_class_color(404, cx),
            status_class_color(200, cx),
            cx.theme().status().http_client_error,
        )
    });
    assert_eq!(
        client_error, ok_tone,
        "a 4xx draws in the client-error status token"
    );
    assert_ne!(client_error, success, "and not in the success tone");
}

#[gpui::test]
fn test_the_headers_and_cookies_tabs_count_what_the_response_carries(cx: &mut TestAppContext) {
    let (view, cx) = viewer(cx);
    assert!(
        bounds(cx, tab_count_selector(1)).is_none(),
        "no counts before a response"
    );
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(b"{}".to_vec()));
    response.headers = vec![
        Header::new("content-type", "application/json"),
        Header::new("set-cookie", "session=example-REDACTED; Path=/"),
        Header::new("set-cookie", "theme=dark; Path=/"),
    ];
    show(&view, response, cx);

    assert!(
        bounds(cx, tab_count_selector(1)).is_some(),
        "Headers shows a count"
    );
    assert!(
        bounds(cx, tab_count_selector(2)).is_some(),
        "Cookies shows a count"
    );
    assert!(
        bounds(cx, tab_count_selector(0)).is_none() && bounds(cx, tab_count_selector(3)).is_none(),
        "Body and Timeline count nothing"
    );
    let counts = cx.update(|_, cx| {
        let cookies = view.read(cx).cookies.read(cx).rows().len();
        let headers = view
            .read(cx)
            .response()
            .map(|response| response.headers.len());
        (headers, cookies)
    });
    assert_eq!(counts, (Some(3), 2), "3 headers, 2 of them cookies");

    view.update(cx, |view, cx| view.clear(cx));
    draw_window(cx);
    assert!(
        bounds(cx, tab_count_selector(1)).is_none(),
        "clearing drops the previous response's counts"
    );
}

#[gpui::test]
fn test_the_body_toolbar_names_the_lines_on_screen(cx: &mut TestAppContext) {
    let (view, cx) = viewer(cx);
    let body = (0..500)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.into_bytes()));
    response.headers = vec![Header::new("content-type", "text/plain")];
    show(&view, response, cx);

    assert!(
        bounds(cx, LINE_RANGE_SELECTOR.to_owned()).is_some(),
        "the readout renders"
    );
    let range = cx.update(|_, cx| view.read(cx).body.read(cx).visible_line_range());
    let (first, last, total) = range.expect("the range is known after layout");
    assert_eq!(first, 1, "the top of the body is on screen");
    assert_eq!(total, 500, "the total counts every line");
    assert!(
        last > first && last < total,
        "only the lines that fit are counted as on screen: {first}–{last} of {total}"
    );
}

#[gpui::test]
fn test_the_empty_state_says_how_to_send_with_the_keymaps_binding(cx: &mut TestAppContext) {
    let (_view, cx) = viewer(cx);
    draw_window(cx);
    assert!(
        bounds(cx, SEND_HINT_SELECTOR.to_owned()).is_some(),
        "the empty state carries its how-to-send line"
    );
    let keys = cx.update(|_, cx| ui::keymap_binding_text(&pustman_actions::request::Send, cx));
    assert!(
        keys.is_some_and(|keys| keys.contains("Enter")),
        "the line names the keymap's Send binding"
    );
}
