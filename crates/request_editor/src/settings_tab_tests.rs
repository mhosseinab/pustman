//! The Settings tab and the TLS badge driven the way a user drives them: the tab
//! clicked in the strip, its controls clicked, and the TLS failure's "Disable
//! verification for this request" offer activated — asserting what lands in the
//! request draft, what a send carries and what renders (S28c). Every assertion is an
//! outcome the user can see or the wire would carry.

use std::sync::Arc;
use std::time::Duration;

use api_model::HttpRequest;
use gpui::{AppContext, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::test_support::{draw_window, init_test};

use super::{VERIFY_TLS_DEFAULT_SELECTOR, VERIFY_TLS_SWITCH_SELECTOR};
use crate::RequestEditor;
use crate::send::{INLINE_ERROR_SELECTOR, TLS_BADGE_SELECTOR, TLS_OFFER_SELECTOR, set_http_client};

/// The draft the tests edit: a GET the URL bar starts out with.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport` (the `send_tests` shape).
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

/// Click one control the way a user does.
fn click_control(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("the control is on screen");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// Open the Settings tab the way a user does: click it in the request tab strip.
fn open_settings_tab(cx: &mut VisualTestContext) {
    click_control("request-tab-settings", cx);
}

/// The `verify_tls` override the request draft carries — what the send is built from.
fn draft_verify_tls(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<bool> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor
                .draft()
                .settings
                .as_ref()
                .and_then(|settings| settings.verify_tls)
        })
    })
}

#[gpui::test]
fn tls_error_offers_disable_and_resend(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let failure = TransportError::Tls {
        reason: String::from("self-signed certificate"),
    };
    transport.push_error(Duration::ZERO, failure.clone());
    transport.push_response(Duration::ZERO, 200, "ok");
    let (editor, cx) = open_editor(cx, &transport);

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let inline = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().cloned()))
        .expect("a TLS failure shows an inline error");
    assert_eq!(
        inline.message,
        failure.user_message(),
        "the message is the transport's own text, verbatim"
    );
    assert_eq!(
        inline.fix,
        failure.suggested_fix(),
        "the fix is the transport's own text, verbatim"
    );
    assert!(
        cx.debug_bounds(TLS_OFFER_SELECTOR).is_some(),
        "a TLS failure offers to disable verification for this request"
    );

    click_control(TLS_OFFER_SELECTOR, cx);

    assert_eq!(
        draft_verify_tls(&editor, cx),
        Some(false),
        "the offer pins verify_tls off on this draft"
    );
    assert_eq!(
        cx.read(|cx| {
            editor.read_with(cx, |editor, cx| editor.settings_tab().read(cx).verify_tls())
        }),
        Some(false),
        "the Settings tab reflects the override the offer set"
    );
    assert!(
        cx.debug_bounds(TLS_BADGE_SELECTOR).is_some(),
        "the warning badge shows while verification is off for this request"
    );
    let sent = transport.requests();
    assert_eq!(sent.len(), 2, "the offer re-sends exactly once");
    assert!(sent[0].settings.verify_tls, "the first send verified TLS");
    assert!(
        !sent[1].settings.verify_tls,
        "the re-send carries verify_tls == false in its SendSettings"
    );

    // A failure that is not a TLS failure shows no offer.
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: String::from("api.example.com"),
        },
    );
    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some(),
        "the failure still shows inline"
    );
    assert!(
        cx.debug_bounds(TLS_OFFER_SELECTOR).is_none(),
        "a non-TLS failure shows no offer"
    );
}

#[gpui::test]
fn tls_badge_visible_while_verify_disabled(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let (editor, cx) = open_editor(cx, &transport);
    assert!(
        cx.debug_bounds(TLS_BADGE_SELECTOR).is_none(),
        "no badge while verification follows the global default"
    );

    open_settings_tab(cx);
    click_control(VERIFY_TLS_SWITCH_SELECTOR, cx);
    assert_eq!(
        draft_verify_tls(&editor, cx),
        Some(false),
        "the switch pins verification off for this request"
    );
    assert!(
        cx.debug_bounds(TLS_BADGE_SELECTOR).is_some(),
        "toggling verify off in the Settings tab shows the badge"
    );

    click_control(VERIFY_TLS_DEFAULT_SELECTOR, cx);
    assert_eq!(
        draft_verify_tls(&editor, cx),
        None,
        "the request again inherits the global default"
    );
    assert!(
        cx.debug_bounds(TLS_BADGE_SELECTOR).is_none(),
        "back to 'use global default' hides the badge"
    );
}
