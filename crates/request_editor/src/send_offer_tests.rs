//! The TLS failure's "Disable verification for this request" offer (LOW #228): it is a
//! button sized to its label, not a box stretched across the inline error that reads
//! like a text input. Every assertion is geometry a user can see.

use std::sync::Arc;
use std::time::Duration;

use api_model::HttpRequest;
use gpui::{AppContext, Entity, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::send::{INLINE_ERROR_SELECTOR, TLS_OFFER_SELECTOR, set_http_client};

/// The draft the tests send: a GET the URL bar starts out with.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.test/ping");
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`.
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

/// The `request::Send` keystroke on this platform — Cmd on macOS, Ctrl elsewhere.
fn send_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-enter"
    } else {
        "ctrl-enter"
    }
}

#[gpui::test]
fn test_the_tls_offer_is_sized_to_its_label_not_the_error_width(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Tls {
            reason: String::from("self-signed certificate"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport);

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let inline = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().cloned()))
        .expect("a TLS failure shows an inline error");
    assert!(!inline.message.is_empty(), "the failure has a message");

    let offer = cx
        .debug_bounds(TLS_OFFER_SELECTOR)
        .expect("a TLS failure offers to disable verification for this request");
    let error = cx
        .debug_bounds(INLINE_ERROR_SELECTOR)
        .expect("the failure shows inline");
    assert!(
        offer.size.width < error.size.width,
        "the offer is sized to its label, not stretched across the inline error \
         like a text input: offer {:?}, error {:?}",
        offer.size.width,
        error.size.width
    );
}
