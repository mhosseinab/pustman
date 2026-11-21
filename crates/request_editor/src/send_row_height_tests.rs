//! The URL row's controls share one height (#218): the method and the URL field stand
//! on one 32 px line — the pack's `density.compact-row` and preferred toolbar target —
//! and Send, under them, is as tall as the field it sends.

use std::sync::Arc;

use gpui::{AppContext, Bounds, Pixels, TestAppContext, px};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::{SEND_BUTTON_SELECTOR, set_http_client};
use crate::RequestEditor;
use crate::url_bar::{METHOD_CONTROL_SELECTOR, URL_FIELD_SELECTOR};

const ROW_HEIGHT: Pixels = px(32.);

#[gpui::test]
fn test_method_url_field_and_send_share_one_32px_row(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        set_http_client(client, cx);
    });
    let (_editor, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(crate::new_request::fresh_request(), window, cx)
    });
    draw_window(cx);

    let mut bounds = |selector: &'static str| -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is on screen"))
    };
    let method = bounds(METHOD_CONTROL_SELECTOR);
    let field = bounds(URL_FIELD_SELECTOR);
    let send = bounds(SEND_BUTTON_SELECTOR);

    for (name, control) in [("method", method), ("URL field", field), ("Send", send)] {
        assert_eq!(
            control.size.height, ROW_HEIGHT,
            "the {name} control is one 32 px row tall"
        );
    }
    assert_eq!(
        field.origin.y, method.origin.y,
        "the URL field shares the method's top edge"
    );
}
