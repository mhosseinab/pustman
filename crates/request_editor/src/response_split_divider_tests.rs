//! The divider between the request and response panes (#224): a 1 px line a user sees,
//! inside a wider transparent strip a user grabs.

use std::sync::Arc;

use gpui::{AppContext, TestAppContext, px};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::{SPLIT_HANDLE_SELECTOR, SPLIT_LINE_SELECTOR};
use crate::RequestEditor;
use crate::send::set_http_client;

#[gpui::test]
fn test_the_split_draws_a_1px_line_inside_a_wider_grab_strip(cx: &mut TestAppContext) {
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

    let handle = cx
        .debug_bounds(SPLIT_HANDLE_SELECTOR)
        .expect("the split handle is on screen");
    let line = cx
        .debug_bounds(SPLIT_LINE_SELECTOR)
        .expect("the divider line is on screen");
    assert_eq!(line.size.height, px(1.), "the visible divider is 1 px");
    assert!(
        handle.size.height > line.size.height * 4.,
        "the grab strip ({:?}) is wider than the line it carries",
        handle.size.height
    );
    assert!(
        handle.contains(&line.center()),
        "the line sits inside its grab strip"
    );
}
