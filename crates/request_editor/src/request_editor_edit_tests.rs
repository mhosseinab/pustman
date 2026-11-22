//! Editing the request after a failed send (#206): the inline failure under the URL
//! bar describes a request that no longer exists once the user has edited it, so the
//! edit clears it — the response pane keeps the failure as the record of the last
//! send. Driven the way a user drives it: the real `request::Send` keystroke from the
//! default keymap against a `FakeTransport` that scripts the failure, then a real edit.

use std::sync::Arc;
use std::time::Duration;

use api_model::{HttpRequest, Method};
use gpui::{AppContext, Entity, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test, press_key};

use crate::RequestEditor;
use crate::send::{INLINE_ERROR_SELECTOR, set_http_client};

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

/// Send a request that fails, and prove the failure is the inline error on display.
fn fail_a_send(cx: &mut VisualTestContext, editor: &Entity<RequestEditor>) {
    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().is_some())),
        "a failed send shows its inline error to start from"
    );
    assert!(cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some());
}

#[gpui::test]
fn editing_the_url_clears_the_stale_inline_error(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: String::from("api.example.com"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport);
    fail_a_send(cx, &editor);

    // The user goes back to the URL and changes it: the failure on display was about
    // the request as it was, so it must not linger under the field being edited.
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().is_none())),
        "editing the URL clears the stale inline error"
    );
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_none(),
        "and the stale copy is gone from under the URL bar"
    );
}

#[gpui::test]
fn changing_the_method_clears_the_stale_inline_error(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: String::from("api.example.com"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport);
    fail_a_send(cx, &editor);

    // A method change is an edit like any other: the custom-method entry the dropdown's
    // "Custom Method…" item runs, driven with a real token and Enter.
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor
                .url_bar()
                .update(cx, |bar, cx| bar.begin_custom_method(window, cx));
        });
    });
    cx.simulate_input("PATCH");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().method.clone())),
        Method::Patch,
        "the method edit landed on the draft"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().is_none())),
        "changing the method clears the stale inline error"
    );
}
