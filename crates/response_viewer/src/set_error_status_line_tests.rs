//! A failed send shows its failure alone (#126, #185): whatever the previous response's
//! status line said — "200 OK", "184 ms" — must not sit above the error panel when
//! [`ResponseView::set_error`] lands. The editor masks this by clearing the region first
//! (S33n), but any direct caller of `set_error` saw the old response's facts over the
//! new failure; the view clears its own status line.

use gpui::{TestAppContext, VisualTestContext};
use http_engine::{Response, ResponseBody, TransportError};
use ui::test_support::{draw_window, init_test};

use crate::error_panel::error_kind_selector;
use crate::{ResponseView, STATUS_LINE_SELECTOR};

#[gpui::test]
fn set_error_replaces_the_previous_response_status_line(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(200, "OK", ResponseBody::Bytes(b"body-on-screen".to_vec())),
            cx,
        )
    });
    draw_window(cx);
    assert!(
        shown(cx, STATUS_LINE_SELECTOR.to_owned()),
        "the first response's status line is on screen"
    );

    view.update(cx, |view, cx| {
        view.set_error(
            TransportError::Dns {
                host: String::from("api.example.test"),
            },
            cx,
        )
    });
    draw_window(cx);

    assert!(
        !shown(cx, STATUS_LINE_SELECTOR.to_owned()),
        "the failed send leaves no previous status line above its error panel"
    );
    assert!(
        shown(cx, error_kind_selector("Dns")),
        "the failure itself is on screen where the status line was"
    );
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}
