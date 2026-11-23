//! The two REQ-HTTP-09 overrides that had no typed field or row — "keep
//! `Authorization` across redirects" and "disable cookie jar" — driven the way a user
//! drives them: the Settings tab opened, its controls clicked, and a send run —
//! asserting what lands in the request draft and what the wire carries (the
//! `settings_tab_tests` shape). Every assertion is an outcome the user can see or the
//! wire would carry.

use std::sync::Arc;
use std::time::Duration;

use api_model::{HttpRequest, RequestSettings};
use gpui::{
    AppContext, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext, px, size,
};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::{
    DISABLE_COOKIE_JAR_DEFAULT_SELECTOR, DISABLE_COOKIE_JAR_SWITCH_SELECTOR,
    KEEP_AUTHORIZATION_DEFAULT_SELECTOR, KEEP_AUTHORIZATION_SWITCH_SELECTOR,
};
use crate::RequestEditor;
use crate::send::set_http_client;

/// The draft the tests edit: a GET the URL bar starts out with, carrying `settings`.
fn request(settings: Option<RequestSettings>) -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request.settings = settings;
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport` (the `settings_tab_tests` shape).
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
        set_http_client(client, cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    // Room for the whole Settings form. The request pane is one half of the window
    // (`response_split::DEFAULT_SPLIT_FRACTION`), and rows under the pane's bottom
    // edge paint but no longer take clicks — so the window is twice the form's height.
    cx.simulate_resize(size(px(1440.), px(1800.)));
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

/// The [`RequestSettings`] block the request draft carries — what the send is built
/// from. A draft that overrides nothing reads as the empty block.
fn draft_settings(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> RequestSettings {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor.draft().settings.clone().unwrap_or_default()
        })
    })
}

#[gpui::test]
fn keep_authorization_and_disable_cookie_jar_reach_the_send_from_the_request_settings(
    cx: &mut TestAppContext,
) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::ZERO, 200, "ok");
    let settings = RequestSettings {
        keep_authorization: Some(true),
        disable_cookie_jar: Some(true),
        ..Default::default()
    };
    let (editor, cx) = open_editor(cx, &transport, request(Some(settings.clone())));

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "one send ran");
    assert!(
        sent[0].settings.keep_authorization,
        "the send carries the keep-Authorization override"
    );
    assert!(
        sent[0].settings.disable_cookie_jar,
        "the send carries the disable-cookie-jar override"
    );
    assert_eq!(
        draft_settings(&editor, cx),
        settings,
        "the draft keeps the overrides it was opened with"
    );
}

#[gpui::test]
fn the_settings_tab_round_trips_keep_authorization_and_disable_cookie_jar(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let (editor, cx) = open_editor(cx, &transport, request(None));

    open_settings_tab(cx);
    for selector in [
        KEEP_AUTHORIZATION_SWITCH_SELECTOR,
        KEEP_AUTHORIZATION_DEFAULT_SELECTOR,
        DISABLE_COOKIE_JAR_SWITCH_SELECTOR,
        DISABLE_COOKIE_JAR_DEFAULT_SELECTOR,
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "the Settings tab offers {selector}"
        );
    }

    // The switches pin both overrides on the draft.
    click_control(KEEP_AUTHORIZATION_SWITCH_SELECTOR, cx);
    click_control(DISABLE_COOKIE_JAR_SWITCH_SELECTOR, cx);
    let pinned = draft_settings(&editor, cx);
    assert_eq!(
        pinned.keep_authorization,
        Some(true),
        "the switch pins keep_authorization on for this request"
    );
    assert_eq!(
        pinned.disable_cookie_jar,
        Some(true),
        "the switch pins disable_cookie_jar on for this request"
    );

    // The tab re-seeds from the draft when the user comes back to it: one click on a
    // switch that shows on pins `false` — had the re-seed dropped the override, the
    // switch would show off and the click would pin `true`.
    click_control("request-tab-body", cx);
    open_settings_tab(cx);
    click_control(KEEP_AUTHORIZATION_SWITCH_SELECTOR, cx);
    click_control(DISABLE_COOKIE_JAR_SWITCH_SELECTOR, cx);
    let flipped = draft_settings(&editor, cx);
    assert_eq!(
        flipped.keep_authorization,
        Some(false),
        "the tab came back showing the pinned keep_authorization"
    );
    assert_eq!(
        flipped.disable_cookie_jar,
        Some(false),
        "the tab came back showing the pinned disable_cookie_jar"
    );

    // The "use global default" checkboxes put both back to inherit.
    click_control(KEEP_AUTHORIZATION_DEFAULT_SELECTOR, cx);
    click_control(DISABLE_COOKIE_JAR_DEFAULT_SELECTOR, cx);
    let inherited = draft_settings(&editor, cx);
    assert_eq!(
        inherited.keep_authorization, None,
        "the request again inherits the global keep-Authorization default"
    );
    assert_eq!(
        inherited.disable_cookie_jar, None,
        "the request again inherits the global cookie-jar default"
    );
}
