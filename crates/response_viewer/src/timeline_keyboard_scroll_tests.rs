//! Keyboard scrolling in the Timeline tab (S33o fix 2, review 1 #4): the tab is one
//! scrolling document inside its pane, so the keyboard must move its scroll the way the
//! wheel does (keyboard-first non-negotiable, `rust-gpui-ui-ux`). The setup mirrors
//! `timeline_m1_fixes_tests.rs` — a timeline taller than its pane, drawn with
//! `draw_window` — and focuses the tab before each key press.

use std::ops::Deref;
use std::time::Duration;

use api_model::{Method, SecretValues};
use gpui::{
    Bounds, ElementId, Entity, Focusable, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
    ScrollEvent, ScrollSource, TestAppContext, TouchPhase, VisualTestContext, px, size,
};
use http_engine::{
    Header, RedirectHop, RequestBody, ResolvedRequest, Response, ResponseBody, SendSettings,
    Timings,
};
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;

use crate::ResponseView;

use super::TimelineTab;

#[gpui::test]
fn test_page_down_scrolls_the_timeline_inside_its_pane(cx: &mut TestAppContext) {
    let (view, mut cx) = tall_timeline(cx);
    let strip_before = tab_bounds(0, &mut cx).expect("the tab strip is painted before the press");
    let before = scroll_offset(&view, &cx);
    let viewport = viewport_height(&view, &cx);
    assert_eq!(
        before.y,
        px(0.),
        "the timeline starts at the top of its pane"
    );

    press_key(&mut cx, "pagedown");
    cx.run_until_parked();
    draw_window(&mut cx);

    let after = scroll_offset(&view, &cx);
    assert!(
        after.y < before.y,
        "PageDown scrolls the timeline down inside its pane"
    );
    let max = max_offset(&view, &cx).y;
    assert_eq!(
        before.y - after.y,
        viewport.min(max),
        "PageDown moves one viewport height — clamped to the bottom of the content"
    );
    let strip_after = tab_bounds(0, &mut cx).expect("the tab strip is painted after the press");
    assert_eq!(
        strip_before.origin, strip_after.origin,
        "scrolling the timeline from the keyboard never moves the window chrome"
    );
}

#[gpui::test]
fn test_home_returns_the_timeline_to_the_top(cx: &mut TestAppContext) {
    let (view, mut cx) = tall_timeline(cx);
    press_key(&mut cx, "pagedown");
    press_key(&mut cx, "pagedown");
    cx.run_until_parked();
    draw_window(&mut cx);
    assert!(
        scroll_offset(&view, &cx).y < px(0.),
        "two pages down leave the top of the timeline behind"
    );

    press_key(&mut cx, "home");
    cx.run_until_parked();
    draw_window(&mut cx);

    assert_eq!(
        scroll_offset(&view, &cx).y,
        px(0.),
        "Home returns the timeline to the top"
    );
}

#[gpui::test]
fn test_end_reaches_the_bottom_of_the_timeline(cx: &mut TestAppContext) {
    let (view, mut cx) = tall_timeline(cx);
    assert!(
        max_offset(&view, &cx).y > px(0.),
        "the fixture timeline is taller than its pane"
    );

    press_key(&mut cx, "end");
    cx.run_until_parked();
    draw_window(&mut cx);

    let offset = scroll_offset(&view, &cx);
    assert_eq!(
        offset.y,
        -max_offset(&view, &cx).y,
        "End reaches the bottom of the timeline and clamps there"
    );
}

#[gpui::test]
fn test_down_scrolls_the_timeline_one_line_and_up_scrolls_it_back(cx: &mut TestAppContext) {
    let (view, mut cx) = tall_timeline(cx);
    let line = cx.update(|window, _cx| window.line_height());

    press_key(&mut cx, "down");
    cx.run_until_parked();
    draw_window(&mut cx);
    assert_eq!(
        -scroll_offset(&view, &cx).y,
        line,
        "Down scrolls the timeline by one line height"
    );

    press_key(&mut cx, "up");
    cx.run_until_parked();
    draw_window(&mut cx);
    assert_eq!(
        scroll_offset(&view, &cx).y,
        px(0.),
        "Up scrolls the line back and clamps at the top"
    );
}

#[gpui::test]
fn test_tabbing_to_a_hop_scrolls_it_into_view(cx: &mut TestAppContext) {
    // A tab whose redirect list paints its hop rows: in the tall fixture the lists
    // shrink to nothing and no hop is a tab stop at all, so the tab walk is proven on a
    // tab with a scrollable redirect list (`timeline_tests.rs`'s window).
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| view.set_response(&long_response(), cx));
    draw_window(cx);
    let focus = view.read_with(cx, |view, cx| view.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw_window(cx);

    // Scroll the redirect list down a hair: the top hop row is now clipped at the
    // list's edge — still a tab stop, but not fully in view.
    let first_row = cx
        .debug_bounds(leak("timeline-hop-0".to_owned()))
        .expect("the first hop row is painted");
    cx.simulate_mouse_move(first_row.center(), None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_event(ScrollEvent {
        position: first_row.center(),
        delta: ScrollDelta::Pixels(Point::new(px(0.), px(-7.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.run_until_parked();
    draw_window(cx);

    let rows_before: Vec<Option<Bounds<Pixels>>> = (0..30)
        .map(|hop| cx.debug_bounds(leak(format!("timeline-hop-{hop}"))))
        .collect();

    press_key(cx, "tab");
    cx.run_until_parked();
    draw_window(cx);

    let focused_hop = cx.update(|window, cx| {
        view.read(cx)
            .hop_focus
            .iter()
            .position(|focus| focus.contains_focused(window, cx))
    });
    let hop = focused_hop.expect("Tab moves focus into the redirect chain's hop rows");
    let before = rows_before
        .get(hop)
        .copied()
        .flatten()
        .expect("the focused hop's row was painted before the press");
    let after = cx
        .debug_bounds(leak(format!("timeline-hop-{hop}")))
        .expect("the focused hop's row is painted");
    let viewport = cx.update(|_window, cx| view.read(cx).scroll.bounds());
    assert!(
        after.origin.y >= viewport.origin.y && after.bottom() <= viewport.bottom(),
        "the hop row Tab landed on is inside the timeline's viewport (row {after:?} in \
         {viewport:?})"
    );
    assert!(
        after.origin.y > before.origin.y,
        "the clipped row is scrolled fully into view on Tab (before {before:?}, after \
         {after:?})"
    );
}

/// A window whose timeline is taller than its pane (the `timeline_m1_fixes_tests.rs`
/// setup), with the Timeline tab selected, focused and drawn — key handlers register
/// when the focused frame paints (`tab_bar_keyboard_tests.rs`).
fn tall_timeline(cx: &mut TestAppContext) -> (Entity<ResponseView>, VisualTestContext) {
    cx.update(init_test);
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

    // Select the Timeline tab, then focus the tab itself and paint once more.
    let timeline_tab = tab_bounds(3, &mut cx).expect("the Timeline tab is on the strip");
    cx.simulate_click(timeline_tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(&mut cx);
    let focus = view.read_with(&cx, |view, cx| view.timeline.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw_window(&mut cx);

    (view, cx)
}

/// Where the tab at `index` is painted: the `ui::Tab`'s debug selector is its element id
/// (`components/tab.rs`), as `timeline_m1_fixes_tests.rs` reads it.
fn tab_bounds(index: usize, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(leak(ElementId::from(("response-tab", index)).to_string()))
}

/// The timeline's scroll offset, straight from the scroll root's handle.
fn scroll_offset(view: &Entity<ResponseView>, cx: &VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, cx| view.timeline.read(cx).scroll.offset())
}

/// How far the timeline can scroll: the bottom of its content bounds.
fn max_offset(view: &Entity<ResponseView>, cx: &VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, cx| view.timeline.read(cx).scroll.max_offset())
}

/// The height of the timeline's viewport — what PageUp and PageDown move.
fn viewport_height(view: &Entity<ResponseView>, cx: &VisualTestContext) -> Pixels {
    view.read_with(cx, |view, cx| {
        view.timeline.read(cx).scroll.bounds().size.height
    })
}

/// `VisualTestContext::debug_bounds` takes a `&'static str`, so selectors are leaked —
/// fine for a test's short life (the `timeline_tests.rs` pattern).
fn leak(selector: String) -> &'static str {
    selector.leak()
}

/// A response whose timeline is taller than the pane: every measured phase, the network
/// facts and a long redirect chain (the `timeline_m1_fixes_tests.rs` fixture).
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
/// taller than the pane too (the `timeline_m1_fixes_tests.rs` fixture).
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
