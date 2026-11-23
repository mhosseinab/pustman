//! An auth that adds no credential is never sent silently (REQ-HTTP-06): empty Basic
//! fields send without credentials, and the tab says so beside the response and in the
//! Console — the user does not have to guess from a 401.

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{Auth, HttpRequest};
use console::{ConsoleLevel, ConsoleStore};
use gpui::{AppContext, TestAppContext};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::{AUTH_WARNING_SELECTOR, set_http_client};
use crate::RequestEditor;

/// A GET whose Basic auth has both fields empty.
fn empty_basic_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = "https://example.test/private".to_string();
    request.auth = Auth::Basic {
        username: Some(String::new()),
        password: Some(String::new()),
        extra: BTreeMap::new(),
    };
    request
}

#[gpui::test]
fn test_empty_basic_auth_sends_with_a_visible_warning(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 401, "unauthorized");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
        set_http_client(client, cx);
    });
    let (editor, cx) =
        cx.add_window_view(|window, cx| RequestEditor::new(empty_basic_request(), window, cx));

    editor.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(transport.requests().len(), 1, "the send still goes out");
    let warning = cx.read(|cx| editor.read(cx).auth_warning().map(str::to_string));
    let warning = warning.expect("the tab carries the auth warning");
    assert!(
        warning.contains("without credentials"),
        "the warning says the request went without credentials: {warning}"
    );
    assert!(
        cx.debug_bounds(AUTH_WARNING_SELECTOR).is_some(),
        "the auth warning is on screen"
    );
    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert!(
            entries
                .iter()
                .any(|entry| entry.level == ConsoleLevel::Warning && entry.raw_response == warning),
            "the Console records the same warning: {entries:?}"
        );
    });
}

#[gpui::test]
fn test_the_auth_warning_clears_on_the_next_send(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 401, "unauthorized");
    transport.push_response(std::time::Duration::ZERO, 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
        set_http_client(client, cx);
    });
    let (editor, cx) =
        cx.add_window_view(|window, cx| RequestEditor::new(empty_basic_request(), window, cx));
    editor.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).auth_warning().is_some()));

    let auth_tab = cx.read(|cx| editor.read(cx).auth_tab().clone());
    auth_tab.update(cx, |tab, cx| {
        tab.set_auth_type(crate::auth_tab::AuthType::None, cx)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();

    assert!(
        cx.read(|cx| editor.read(cx).auth_warning().is_none()),
        "a send whose auth applied shows no auth warning"
    );
}
