//! The pre-send unresolved-variable warning, driven the way a user drives it: the real
//! `request::Send` keystroke against a request URL carrying `{{missing}}`, then a real
//! click on the bar's "Send anyway". Every assertion is an outcome the user can see —
//! the names the bar lists, the request that reached (or never reached) the transport,
//! and the warning the Console entry carries.

use std::sync::Arc;

use api_model::HttpRequest;
use gpui::{AppContext, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::{SEND_ANYWAY_SELECTOR, UNRESOLVED_WARNING_SELECTOR};
use crate::RequestEditor;
use crate::send::set_http_client;

/// The draft the tests send: a GET whose URL names a variable nothing defines.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/{{missing}}");
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the Console installed so the send's
/// outcome has somewhere to land.
fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
) -> (Entity<RequestEditor>, &'a mut VisualTestContext) {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
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
fn unresolved_warning_lists_names_and_allows_send(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport);

    // Send is held back: the bar names the variable and nothing reaches the wire.
    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        transport.requests().len(),
        0,
        "an unresolved variable holds the send back"
    );
    let warning = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().cloned()))
        .expect("the held-back send shows the warning bar");
    assert_eq!(
        warning.names(),
        &[String::from("missing")],
        "the bar lists the name nothing resolved"
    );
    assert!(
        cx.debug_bounds(UNRESOLVED_WARNING_SELECTOR).is_some(),
        "the bar is on screen, above the URL bar"
    );

    // "Send anyway" sends exactly once, and the warning travels with the outcome.
    let bounds = cx
        .debug_bounds(SEND_ANYWAY_SELECTOR)
        .expect("the Send anyway button is on screen");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        transport.requests().len(),
        1,
        "Send anyway sends exactly one request"
    );
    let logged = cx.read(|cx| {
        cx.global::<console::ConsoleStore>()
            .entries()
            .iter()
            .any(|entry| entry.raw_response.contains("Unresolved variables: missing"))
    });
    assert!(
        logged,
        "the Console entry carries the unresolved-variables warning"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().is_none())),
        "the bar is gone once the request has been sent"
    );
    assert!(cx.debug_bounds(UNRESOLVED_WARNING_SELECTOR).is_none());
}
