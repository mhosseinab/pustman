//! The Timeline tab draws a bar per phase the transport *measured* and nothing at all for
//! one it could not (REQ-NET-01, ADR-0003: "absent rather than zeroed"), and it lists every
//! hop of the redirect chain with the facts the transport recorded (REQ-NET-03) — a dropped
//! hop would hide a request the user's own send made.

use std::time::Duration;

use api_model::Method;
use gpui::TestAppContext;
use http_engine::{Header, RedirectHop, Response, ResponseBody, Timings};
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;

use super::TimelineTab;

#[gpui::test]
fn absent_phases_not_rendered(cx: &mut TestAppContext) {
    cx.update(init_test);
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    // The transport measures around reqwest's futures (ADR-0003): only these three carry
    // a value, while the four phases inside the connect call stay `None`.
    response.timings = Timings {
        first_byte: Some(Duration::from_millis(120)),
        download: Some(Duration::from_millis(80)),
        total: Some(Duration::from_millis(200)),
        ..Timings::default()
    };
    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| view.set_response(&response, cx));
    draw_window(cx);

    let labels: Vec<String> = view.read_with(cx, |view, _| {
        view.phase_rows()
            .iter()
            .map(|row| row.label.to_string())
            .collect()
    });
    assert_eq!(
        labels,
        ["Time to first byte", "Download", "Total"],
        "a phase the transport did not measure gets no row at all"
    );

    for slug in ["first-byte", "download", "total"] {
        assert!(
            cx.debug_bounds(leak(format!("timeline-bar-{slug}")))
                .is_some(),
            "the measured phase {slug} draws its bar"
        );
    }
    for slug in ["prepare", "dns", "connect", "tls"] {
        assert!(
            cx.debug_bounds(leak(format!("timeline-bar-{slug}")))
                .is_none(),
            "the unmeasured phase {slug} draws no bar — not even a zero one"
        );
    }

    // The absent phases are not silently dropped: the note names them and says why
    // ("absent rather than zeroed", ADR-0003).
    assert!(
        cx.debug_bounds("timeline-unmeasured-note").is_some(),
        "the note under the phases explains what is absent and why"
    );
}

#[gpui::test]
fn redirect_chain_lists_every_hop(cx: &mut TestAppContext) {
    cx.update(init_test);
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.redirects = vec![
        hop(
            "https://example.test/start",
            301,
            "https://example.test/second",
            12,
        ),
        hop(
            "https://example.test/second",
            302,
            "https://example.test/third",
            30,
        ),
        hop(
            "https://example.test/third",
            307,
            "https://example.test/final",
            5,
        ),
    ];
    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| view.set_response(&response, cx));
    draw_window(cx);

    let rows: Vec<(String, String, String, String, String)> = view.read_with(cx, |view, _| {
        view.hop_rows()
            .iter()
            .map(|hop| {
                (
                    hop.method.to_string(),
                    hop.url.to_string(),
                    hop.status.to_string(),
                    hop.location.to_string(),
                    hop.timing.to_string(),
                )
            })
            .collect()
    });
    assert_eq!(rows.len(), 3, "every hop of the chain gets a row");

    let start = rows.first().expect("the first hop gets a row");
    assert_eq!(start.0, "GET", "the hop's method reaches the row");
    assert_eq!(
        start.1, "https://example.test/start",
        "the hop's URL reaches the row"
    );
    assert_eq!(start.2, "301", "the hop's status reaches the row");
    assert_eq!(
        start.3, "https://example.test/second",
        "the Location that was followed reaches the row"
    );
    assert_eq!(start.4, "12 ms", "the hop's timing reaches the row");

    let last = rows
        .get(2)
        .expect("the last hop gets a row like every other one");
    assert_eq!(last.1, "https://example.test/third");
    assert_eq!(last.2, "307");
    assert_eq!(last.3, "https://example.test/final");
    assert_eq!(last.4, "5 ms");

    for hop in 0..3 {
        assert!(
            cx.debug_bounds(leak(format!("timeline-hop-{hop}")))
                .is_some(),
            "hop {hop} of the chain is on screen"
        );
    }

    // A hop's response headers sit behind its keyboard-reachable toggle: Enter on the
    // focused toggle puts them in the list under that hop (the step caps this file at its
    // two named tests, so the expandable detail is asserted here).
    let toggle = view
        .read_with(cx, |view, _| view.hop_focus.get(1).cloned())
        .expect("every hop has its toggle's focus handle");
    cx.update(|window, cx| window.focus(&toggle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    draw_window(cx);
    assert!(
        cx.debug_bounds("timeline-hop-1-header-0").is_some(),
        "the expanded hop shows its response headers in the list"
    );
    assert!(
        cx.debug_bounds("timeline-hop-0-header-0").is_none(),
        "a hop that was not expanded shows none of its headers"
    );
}

/// One hop of a chain, as the transport records it.
fn hop(url: &str, status: u16, location: &str, millis: u64) -> RedirectHop {
    RedirectHop {
        method: Method::Get,
        url: Url::parse(url).expect("the fixture URL must parse"),
        status,
        headers: vec![Header::new("content-type", "application/json")],
        location: Some(Url::parse(location).expect("the fixture Location must parse")),
        timing: Some(Duration::from_millis(millis)),
    }
}

/// `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is leaked —
/// fine for a test's short life (the `error_panel_tests.rs` pattern).
fn leak(selector: String) -> &'static str {
    selector.leak()
}
