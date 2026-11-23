//! Request tabs send independently through the app's one client (REQ-HTTP-12): a send
//! in one tab never cancels another tab's, and a cancel in one tab leaves the others'
//! sends running. And the pre-send check covers the auth fields: a Bearer token whose
//! `{{token}}` nothing resolves raises the unresolved-variable bar like any other field
//! (REQ-VAR-07).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Auth, HttpRequest};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::set_http_client;
use crate::RequestEditor;
use crate::unresolved_warning::UNRESOLVED_WARNING_SELECTOR;

/// A plain GET to `url`.
fn request(url: &str) -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = url.to_string();
    request
}

/// The app's init order with one client backed by `transport`, shared by every tab.
fn init(cx: &mut TestAppContext, transport: &FakeTransport) {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
        set_http_client(client, cx);
    });
}

/// Two request tabs in one window, sending through the one shared client.
fn open_two_tabs<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
) -> (
    Entity<RequestEditor>,
    Entity<RequestEditor>,
    &'a mut VisualTestContext,
) {
    init(cx, transport);
    let (first, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(request("https://example.test/slow"), window, cx)
    });
    let second = cx.update(|window, cx| {
        cx.new(|cx| RequestEditor::new(request("https://example.test/fast"), window, cx))
    });
    (first, second, cx)
}

/// The status code the tab's status line reports, if a response landed.
fn status(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<u16> {
    cx.read(|cx| editor.read(cx).status_line().map(|line| line.status))
}

#[gpui::test]
fn test_two_tabs_sending_at_once_each_get_their_own_response(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(2), 200, "slow");
    transport.push_response(Duration::from_secs(1), 201, "fast");
    let (first, second, cx) = open_two_tabs(cx, &transport);

    first.update(cx, |editor, cx| editor.start_send(cx));
    second.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| first.read(cx).is_sending()));
    assert!(cx.read(|cx| second.read(cx).is_sending()));

    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();

    assert_eq!(
        status(&first, cx),
        Some(200),
        "the first tab gets its own response, not \"Cancelled\""
    );
    assert_eq!(status(&second, cx), Some(201));
    assert_eq!(transport.dropped(), 0, "neither tab's send was cancelled");
}

#[gpui::test]
fn test_cancelling_one_tab_leaves_the_other_tabs_send_running(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(1), 200, "cancelled");
    transport.push_response(Duration::from_secs(1), 202, "kept");
    let (first, second, cx) = open_two_tabs(cx, &transport);

    first.update(cx, |editor, cx| editor.start_send(cx));
    second.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();
    first.update(cx, |editor, cx| editor.cancel_send(cx));
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    assert_eq!(
        status(&first, cx),
        None,
        "the cancelled tab shows no response"
    );
    assert_eq!(
        status(&second, cx),
        Some(202),
        "the other tab's send ran to its response"
    );
    assert_eq!(
        transport.dropped(),
        1,
        "only the cancelled tab's send was dropped"
    );
}

#[gpui::test]
fn test_an_unresolved_bearer_token_raises_the_warning_bar_naming_it(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    init(cx, &transport);
    let mut draft = request("https://example.test/me");
    draft.auth = Auth::Bearer {
        token: Some("{{token}}".to_string()),
        extra: BTreeMap::new(),
    };
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(draft, window, cx));

    editor.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();
    draw_window(cx);

    let names = cx.read(|cx| {
        editor
            .read(cx)
            .unresolved_warning()
            .map(|warning| warning.names().to_vec())
    });
    assert_eq!(
        names,
        Some(vec!["token".to_string()]),
        "the bar names the auth field's unresolved variable"
    );
    assert!(
        cx.debug_bounds(UNRESOLVED_WARNING_SELECTOR).is_some(),
        "the unresolved-variable bar is on screen"
    );
    assert_eq!(
        transport.requests().len(),
        0,
        "the send is held back until the user chooses Send anyway"
    );
}
