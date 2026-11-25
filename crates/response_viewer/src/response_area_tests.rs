//! The response area as the approved mockup draws it (S48u5): the header reads
//! `Response  [404 Not Found]  Time 12 ms · Size 9 B  …  Save response`, with the pill
//! in its HTTP class's status tone; the Headers and Cookies tabs count what the response
//! carries; the body toolbar says which lines are on screen; and the empty state says
//! how to send.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Bounds, ElementId, Entity, Pixels, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::{
    SAVE_RESPONSE_SELECTOR, status_meta_selector, status_pill_selector, status_pill_tone_selector,
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

    // The tone is read back from what the pill was drawn with, then held to the theme's
    // client-error token — and shown not to be the success one.
    let (client_error, success) = cx.update(|_, cx| {
        let status = cx.theme().status();
        (status.http_client_error, status.http_success)
    });
    assert!(
        bounds(cx, status_pill_tone_selector(404, client_error)).is_some(),
        "the 404 pill's text is drawn in the client-error status token"
    );
    assert!(
        bounds(cx, status_pill_tone_selector(404, success)).is_none(),
        "and not in the success tone"
    );
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
    let labels = cx.update(|_, cx| view.read(cx).tab_bar.read(cx).accessible_labels());
    assert_eq!(
        labels,
        vec!["Body", "Headers, 3", "Cookies, 2", "Timeline"],
        "the tabs read the fixture's 3 headers, 2 of them Set-Cookie"
    );

    view.update(cx, |view, cx| view.clear(cx));
    draw_window(cx);
    assert!(
        bounds(cx, tab_count_selector(1)).is_none(),
        "clearing drops the previous response's counts"
    );
    let labels = cx.update(|_, cx| view.read(cx).tab_bar.read(cx).accessible_labels());
    assert_eq!(labels, vec!["Body", "Headers", "Cookies", "Timeline"]);
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
    let shown = |cx: &mut VisualTestContext, text: &str| {
        bounds(cx, format!("{LINE_RANGE_SELECTOR}-{text}")).is_some()
    };
    let (_, last, _) = cx
        .update(|_, cx| view.read(cx).body.read(cx).visible_line_range())
        .expect("the range is known after layout");
    assert!(last > 1 && last < 500, "only the lines that fit: 1–{last}");
    assert!(
        shown(cx, &format!("Lines 1–{last} of 500")),
        "the toolbar draws the range from the top"
    );

    // Jump the way End, find and a scrollbar drag do: the scroll handle moves outside
    // this view's own render.
    let body = cx.update(|_, cx| view.read(cx).body.clone());
    let notified = Rc::new(Cell::new(0));
    let _observer = cx.update(|_, cx| {
        let notified = Rc::clone(&notified);
        cx.observe(&body, move |_, _| notified.set(notified.get() + 1))
    });
    cx.update(|_, cx| {
        body.read(cx)
            .scroll_handle_for_test()
            .scroll_to_item_strict(300, gpui::ScrollStrategy::Top)
    });
    draw_window(cx);
    assert!(
        notified.get() > 0,
        "the frame that applied the jump asks the body to redraw its readout"
    );
    draw_window(cx);
    let height = last - 1;
    assert!(
        shown(cx, &format!("Lines 301–{} of 500", 301 + height)),
        "after the jump the readout names the lines now on screen"
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
    let keys = cx
        .update(|_, cx| ui::keymap_binding_text(&pustman_actions::request::Send, cx))
        .filter(|keys| !keys.is_empty());
    let Some(keys) = keys else {
        panic!("the default keymap binds request::Send");
    };
    assert!(
        bounds(
            cx,
            format!("{SEND_HINT_SELECTOR}-Press {keys} or click Send.")
        )
        .is_some(),
        "the line names the keymap's Send binding, {keys}"
    );
}
