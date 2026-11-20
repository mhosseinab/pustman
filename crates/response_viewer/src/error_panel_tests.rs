//! A transport failure is not an empty response (REQ-RESP-08, PRD §8.6): the panel
//! names the error's type and shows `TransportError`'s own message and suggested fix in
//! place of the body — and the "Open in Console" link stays hidden while `console::Focus`
//! does not exist, because a link that goes nowhere is a stub.

use gpui::{TestAppContext, VisualTestContext};
use http_engine::{Response, ResponseBody, TransportError};
use ui::test_support::{draw_window, init_test};

use super::{
    OPEN_CONSOLE_SELECTOR, error_fix_selector, error_kind_selector, error_message_selector,
};
use crate::{BODY_PANE_SELECTOR, ResponseView};

#[gpui::test]
fn transport_error_renders_panel_with_fix(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));

    // A response with a body is on screen first, so "the body is not shown" later is a
    // real replacement and not a region that was never filled.
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(200, "OK", ResponseBody::Bytes(b"body-on-screen".to_vec())),
            cx,
        )
    });
    draw_window(cx);
    assert!(
        shown(cx, BODY_PANE_SELECTOR.to_owned()),
        "the response body is on screen before the failure"
    );

    let error = TransportError::Dns {
        host: "api.example.test".to_owned(),
    };
    view.update(cx, |view, cx| view.set_error(error.clone(), cx));
    draw_window(cx);

    assert!(
        shown(cx, error_kind_selector("Dns")),
        "the panel names the error's type"
    );
    assert!(
        shown(cx, error_message_selector(&error.user_message())),
        "the panel shows the message"
    );
    assert!(
        shown(cx, error_fix_selector(error.suggested_fix())),
        "the panel shows the suggested fix"
    );
    assert!(
        !shown(cx, BODY_PANE_SELECTOR.to_owned()),
        "the error replaces the body: the body is not shown"
    );
    assert!(
        !shown(cx, OPEN_CONSOLE_SELECTOR.to_owned()),
        "the \"Open in Console\" link stays hidden while console::Focus does not exist"
    );
}

/// Whether anything is on screen under this debug key (the `panel_tests.rs` pattern:
/// `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is leaked —
/// fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}
