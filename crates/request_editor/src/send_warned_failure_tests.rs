//! A "Send anyway" send that ends in a transport error records its unresolved-variable
//! warning to the Console with the failure (REQ-VAR-07, #188): the warned send's names
//! reach the Console whether the send succeeded or not. Driven the way a user drives it
//! — the real `request::Send` keystroke, the real warning bar's "Send anyway" click —
//! against a `FakeTransport` that scripts the failure.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{HttpRequest, KeyValue};
use console::{ConsoleLevel, ConsoleStore};
use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test};

use super::INLINE_ERROR_SELECTOR;
use crate::RequestEditor;
use crate::unresolved_warning::SEND_ANYWAY_SELECTOR;

/// The draft under test: a plain GET whose header references `{{missing}}`, a name no
/// scope defines — what makes the pre-send check hold the send back (REQ-VAR-07).
fn unresolved_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request.headers.push(KeyValue {
        key: String::from("X-Trace"),
        value: Some(String::from("{{missing}}")),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    });
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the Console installed so every send's
/// entry has somewhere to land.
fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &'a mut VisualTestContext) {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
        super::set_http_client(client, cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
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
    if os == gpui::OperatingSystem::Mac {
        "cmd-enter"
    } else {
        "ctrl-enter"
    }
}

#[gpui::test]
fn test_send_anyway_failure_logs_the_unresolved_warning(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Connect {
            address: String::from("127.0.0.1:8080"),
        },
    );
    let (_editor, cx) = open_editor(cx, &transport, unresolved_request());

    // The pre-send check holds the send back and names `missing` in the bar.
    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        transport.requests().len(),
        0,
        "the send is held back while the names are unresolved"
    );

    // The bar's "Send anyway" sends with the names left literal — and the transport
    // fails the send.
    let button = cx
        .debug_bounds(SEND_ANYWAY_SELECTOR)
        .expect("the warning bar's Send anyway button is on screen");
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(transport.requests().len(), 1, "the send went out");
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some(),
        "the failure still shows inline, under the URL bar"
    );
    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert!(
            entries
                .iter()
                .any(|entry| entry.level == ConsoleLevel::Error),
            "the failure records its Console Error entry, got {entries:?}"
        );
        assert!(
            entries.iter().any(|entry| {
                entry.level == ConsoleLevel::Warning
                    && entry.raw_response == "Unresolved variables: missing"
            }),
            "the warned failure records the unresolved names to the Console (REQ-VAR-07), \
             got {entries:?}"
        );
    });
}
