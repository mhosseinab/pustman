//! The response view embedded under the request editor (S29e), driven the way a user
//! drives it: a send on the real `request::Send` keystroke, and the result landing in
//! the editor's embedded [`response_viewer::ResponseView`] instead of the editor's own
//! status row. Every assertion is an outcome the user can see on screen.
//!
//! `Decision:` `ResponseView` exposes no response getter and this step may add only a
//! status-line debug selector to `response_viewer`, so "the view holds the response" is
//! asserted through what it draws (its status line and body pane render only while it
//! holds one), the response facts `RequestEditor::status_line()` records from the very
//! [`http_engine::Response`] handed to it, and — through `response_view()` — the drawn
//! panes being that entity's, by replacing what it holds.

use std::sync::Arc;

use api_model::HttpRequest;
use gpui::{AppContext, Entity, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::send::{STATUS_LINE_SELECTOR, set_http_client};

/// The `response_viewer` body pane's debug key. That crate's own `BODY_PANE_SELECTOR`
/// is crate-private and this step's authorized change to `response_viewer` covers the
/// status line only (see the report's `Decision:`), so the key is spelled out here.
const BODY_PANE_SELECTOR: &str = "response-body-pane";

/// The draft the tests send: a plain GET the URL bar starts out with (the `send_tests`
/// shape — those helpers are module-private and their file is read-only).
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the cursor where a user starts: the URL
/// field.
fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
) -> (Entity<RequestEditor>, &'a mut VisualTestContext) {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        set_http_client(client, cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    (editor, cx)
}

/// The `request::Send` keystroke on this platform — Cmd on macOS, Ctrl elsewhere,
/// exactly as `assets/keymaps/` binds it.
fn send_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-enter"
    } else {
        "ctrl-enter"
    }
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

#[gpui::test]
fn send_result_appears_in_embedded_response_view(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport);
    draw_window(cx);

    // Before any send the editor's own status row is gone: the selector that row
    // rendered under before S29e finds nothing here, and the embedded viewer shows its
    // empty state instead of a body.
    assert!(
        !shown(cx, STATUS_LINE_SELECTOR),
        "no status line before the send — the editor's own status row is gone"
    );
    assert!(
        !shown(cx, BODY_PANE_SELECTOR),
        "the embedded viewer shows its empty state before the send, not a body"
    );

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    // The response the send produced — 200 with an 11-byte body — landed in the
    // embedded ResponseView: its status line (drawn only while it holds a response) and
    // its body pane are on screen.
    assert!(
        shown(cx, STATUS_LINE_SELECTOR),
        "the embedded viewer's status line is on screen for the response"
    );
    assert!(
        shown(cx, BODY_PANE_SELECTOR),
        "the embedded viewer's body pane is drawn for the response body"
    );
    let status = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.status_line().cloned()))
        .expect("the completed send is recorded");
    assert_eq!(status.status, 200, "the response's status is 200");
    assert_eq!(
        status.size,
        Some(11),
        "the response body is the 11-byte one the transport scripted"
    );

    // The panes above are the embedded view's — the very entity `response_view()` hands
    // out: replacing what it holds replaces what is drawn (its error panel takes the
    // body's place, REQ-RESP-08).
    let response_view =
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.response_view().clone()));
    response_view.update(cx, |view, cx| {
        view.set_error(
            TransportError::Dns {
                host: String::from("api.example.test"),
            },
            cx,
        );
    });
    draw_window(cx);
    assert!(
        !shown(cx, BODY_PANE_SELECTOR),
        "the body pane on screen belongs to the embedded view — its error replaced it"
    );
}
