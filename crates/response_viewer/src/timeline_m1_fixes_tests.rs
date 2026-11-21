//! M1 fixes (S33o): the Timeline tab must scroll inside its own pane, never push or
//! scroll the window around it (plan issue #202). With a long timeline, selecting the
//! tab used to overflow the pane and shift the whole window content up (clipping the tab
//! strip); the tab strip's painted origin is the observable that proves the window chrome
//! never moves, and the scroll check proves the content moved *inside* the pane instead.

use std::ops::Deref;
use std::time::Duration;

use api_model::{Method, SecretValues};
use gpui::{
    Bounds, ElementId, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, ScrollSource,
    TestAppContext, TouchPhase, VisualTestContext, px, size,
};
use http_engine::{
    Header, RedirectHop, RequestBody, ResolvedRequest, Response, ResponseBody, SendSettings, Timings,
};
use ui::test_support::{draw_window, init_test};
use url::Url;

use crate::ResponseView;

#[gpui::test]
async fn test_timeline_tab_does_not_move_the_window_chrome(cx: &mut TestAppContext) {
    cx.update(init_test);
    // A window small enough that a long timeline is taller than the response region
    // below the tab strip — the pane size the S38 hand check ran into.
    let window = cx.open_window(size(px(800.), px(400.)), |_, cx| ResponseView::new(cx));
    let view = window
        .entity(cx)
        .expect("the window holds the response view");
    let mut cx = VisualTestContext::from_window(*window.deref(), cx);
    cx.update(|window, _cx| window.activate_window());

    let response = long_response();
    let request = long_request();
    view.update(&mut cx, |view, cx| {
        view.set_response(response.clone(), cx);
        view.timeline.update(cx, |timeline, cx| {
            timeline.set_request_as_sent(&request, &SecretValues::new(), None, &response, cx);
        });
    });
    draw_window(&mut cx);

    let strip_before = tab_bounds(0, &mut cx).expect("the tab strip is painted before the switch");

    let timeline_tab = tab_bounds(3, &mut cx).expect("the Timeline tab is on the strip");
    cx.simulate_click(timeline_tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(&mut cx);

    let strip_after = tab_bounds(0, &mut cx).expect("the tab strip is painted after the switch");
    assert_eq!(
        strip_before.origin, strip_after.origin,
        "selecting the Timeline tab does not move the tab strip (the window chrome)"
    );

    // The long timeline must scroll *inside* its pane: one scroll notch over the note
    // moves the timeline content up, while the chrome stays where it was.
    let note_before = cx
        .debug_bounds("timeline-unmeasured-note")
        .expect("the phases note is painted");
    // The mouse must be over the pane for the scroll to hit it (hit-testing).
    cx.simulate_mouse_move(note_before.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_event(ScrollEvent {
        position: note_before.center(),
        delta: ScrollDelta::Lines(Point::new(0., -3.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.run_until_parked();
    draw_window(&mut cx);

    let note_after = cx
        .debug_bounds("timeline-unmeasured-note")
        .expect("the phases note is painted after the scroll");
    assert!(
        note_after.origin.y < note_before.origin.y,
        "the timeline scrolls inside its pane (the note moved up: {note_before:?} -> \
         {note_after:?})"
    );
    let strip_after_scroll = tab_bounds(0, &mut cx).expect("the tab strip is painted");
    assert_eq!(
        strip_before.origin, strip_after_scroll.origin,
        "scrolling the timeline never moves the window chrome"
    );
}

/// Where the tab at `index` is painted: the `ui::Tab`'s debug selector is its element id
/// (`components/tab.rs`).
fn tab_bounds(index: usize, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(leak(ElementId::from(("response-tab", index)).to_string()))
}

/// `VisualTestContext::debug_bounds` takes a `&'static str`, so selectors are leaked —
/// fine for a test's short life (the `timeline_tests.rs` pattern).
fn leak(selector: String) -> &'static str {
    selector.leak()
}

/// A response whose timeline is taller than the pane: every measured phase, the network
/// facts and a long redirect chain.
fn long_response() -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.timings = Timings {
        prepare: Some(Duration::from_millis(5)),
        dns: Some(Duration::from_millis(12)),
        connect: Some(Duration::from_millis(30)),
        tls: Some(Duration::from_millis(45)),
        first_byte: Some(Duration::from_millis(120)),
        download: Some(Duration::from_millis(80)),
        total: Some(Duration::from_millis(292)),
        ..Timings::default()
    };
    response.redirects = (0..30)
        .map(|hop_index| RedirectHop {
            method: Method::Get,
            url: Url::parse(&format!("https://example.test/hop/{hop_index}"))
                .expect("the fixture hop URL parses"),
            status: 302,
            headers: vec![Header::new("location", "https://example.test/next")],
            location: Some(
                Url::parse("https://example.test/next").expect("the fixture Location parses"),
            ),
            timing: Some(Duration::from_millis(10)),
        })
        .collect();
    response
}

/// A request whose "Request as sent" record is long: many headers, so that section is
/// taller than the pane too.
fn long_request() -> ResolvedRequest {
    let headers = (0..40)
        .map(|row| Header::new(format!("x-header-{row}"), format!("value {row}")))
        .collect();
    ResolvedRequest::new(
        Method::Get,
        "https://example.test/long",
        headers,
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("the fixture request URL parses")
}
