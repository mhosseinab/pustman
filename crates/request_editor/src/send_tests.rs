//! Sending and cancelling, driven the way a user drives them: the real `request::Send`
//! and `request::Cancel` keystrokes from the default keymap, against a `FakeTransport`
//! that scripts the server's answer. Every assertion is an outcome the user can see —
//! the request that reached the transport, the status line it produced, the button
//! toggling to Cancel, and a failure's inline message and fix.

use std::sync::Arc;
use std::time::Duration;

use api_model::HttpRequest;
use gpui::{AppContext, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test};

use super::{
    CANCEL_BUTTON_SELECTOR, INLINE_ERROR_SELECTOR, SEND_BUTTON_SELECTOR, STATUS_LINE_SELECTOR,
    set_http_client,
};
use crate::RequestEditor;

/// The draft the tests send: a plain GET the URL bar starts out with.
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

#[gpui::test]
fn ctrl_enter_sends_via_fake_transport(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport);

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "ctrl-enter sends exactly one request");
    assert_eq!(
        sent[0].url.as_str(),
        "https://example.com/ping",
        "the request that went out is the draft in the URL bar"
    );

    let status = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.status_line().cloned()))
        .expect("the completed send lands in the status line");
    assert_eq!(status.status, 200);
    assert_eq!(
        status.size,
        Some(11),
        "the status line reports the size of the body that came back"
    );
    assert!(
        cx.debug_bounds(STATUS_LINE_SELECTOR).is_some(),
        "the status line is on screen"
    );
}

#[gpui::test]
fn esc_cancels_in_flight_send(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let (editor, cx) = open_editor(cx, &transport);

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.is_sending())),
        "the send is in flight while the transport has not answered"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, _| editor.is_sending())),
        "Escape clears the in-flight state"
    );
    assert_eq!(
        transport.dropped(),
        1,
        "cancelling drops the send, which aborts the connection"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().is_none())),
        "a user cancel is not an error"
    );
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_none(),
        "nothing is shown inline for a user cancel"
    );
}

#[gpui::test]
fn send_button_toggles_to_cancel_while_in_flight(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let (editor, cx) = open_editor(cx, &transport);
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.send_button_label())),
        "Send"
    );
    assert!(cx.debug_bounds(SEND_BUTTON_SELECTOR).is_some());
    assert!(cx.debug_bounds(CANCEL_BUTTON_SELECTOR).is_none());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.send_button_label())),
        "Cancel",
        "while the request is in flight the Send button renders as Cancel"
    );
    assert!(cx.debug_bounds(CANCEL_BUTTON_SELECTOR).is_some());
    assert!(cx.debug_bounds(SEND_BUTTON_SELECTOR).is_none());

    // The button does what its label says: clicking it cancels the send.
    let bounds = cx
        .debug_bounds(CANCEL_BUTTON_SELECTOR)
        .expect("the Cancel button is on screen");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.send_button_label())),
        "Send",
        "cancelling turns the button back into Send"
    );
    assert_eq!(
        transport.dropped(),
        1,
        "the click cancelled the in-flight send"
    );
}

#[gpui::test]
fn error_shows_inline_message_and_fix(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let failure = TransportError::Dns {
        host: String::from("api.example.com"),
    };
    transport.push_error(Duration::ZERO, failure.clone());
    let (editor, cx) = open_editor(cx, &transport);

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let inline = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().cloned()))
        .expect("a failed send shows an inline error");
    assert_eq!(
        inline.message,
        failure.user_message(),
        "the message is the transport's own text, verbatim"
    );
    assert!(
        inline.message.contains("api.example.com"),
        "the message names the host that did not resolve"
    );
    assert_eq!(
        inline.fix,
        failure.suggested_fix(),
        "the fix is the transport's own text, verbatim"
    );
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some(),
        "the failure is shown inline, under the URL bar"
    );
}
