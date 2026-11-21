//! Sending through the full pipeline with the editor's scopes (S33e): one variable set
//! — [`RequestEditor::set_scopes`] — feeds the highlighting, the pre-send check and the
//! request on the wire; the Auth tab's credentials reach the wire; and every finished or
//! failed send lands exactly its one Console entry. Driven the way a user drives them:
//! the real `request::Send` keystroke and the real cURL paste against a `FakeTransport`
//! that scripts the server's answer, asserting only what the user can see — the request
//! that reached the transport, the warning bar, the inline error and the Console entries.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Body, HttpRequest, Method, RawLanguage, Variable, VariableKind};
use console::{ConsoleLevel, ConsoleStore};
use gpui::{
    AppContext, ClipboardItem, Entity, Focusable, HighlightStyle, Modifiers, OperatingSystem,
    TestAppContext, VisualTestContext,
};
use http_engine::{FakeTransport, HttpClient, RequestBody, TransportError};
use text_input::Highlight;
use ui::prelude::ActiveTheme;
use ui::test_support::{draw_window, init_test};
use variables::{ScopeKind, ScopeStack};

use super::{INLINE_ERROR_SELECTOR, set_http_client};
use crate::RequestEditor;
use crate::auth_tab::AuthType;
use crate::unresolved_warning::UNRESOLVED_WARNING_SELECTOR;

/// The bearer token the Auth tab types in — an obviously fake fixture, so secret
/// scanners and reviewers do not trip on it (the `auth_tab` tests' rule).
const LITERAL_TOKEN: &str = "token-literal-example";

/// The draft most tests send: a plain GET with no variables of its own.
fn plain_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request
}

/// The draft with `{{variables}}` in its URL and raw JSON body — what only the editor's
/// scopes can resolve.
fn templated_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request.url = String::from("{{base}}/users");
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from("{\"name\":\"{{name}}\"}"),
        extra: BTreeMap::new(),
    };
    request
}

/// One enabled variable as a scope records it — a fixture value like `example.test`,
/// never a real secret.
fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// The scopes the tests resolve against — what the M2 environment wire (S43w) will push
/// through `RequestEditor::set_scopes`.
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("base", "http://example.test"),
            variable("name", "Ada"),
        ],
    );
    scopes
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the Console installed so every send's
/// outcome has somewhere to land and the cursor where a user starts: the URL field.
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
        set_http_client(client, cx);
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

/// The `text_input::Paste` keystroke on this platform, exactly as `text_input::init`
/// binds it.
fn paste_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

/// The spans on the URL field — what `{{variable}}` highlighting painted.
fn url_highlights(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<Highlight> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .url_input()
                .read(cx)
                .highlights()
                .to_vec()
        })
    })
}

#[gpui::test]
fn test_send_resolves_variables_from_the_editor_scopes(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport, templated_request());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.set_scopes(scopes(), cx));
    });

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "the send goes out exactly once");
    assert_eq!(
        sent[0].url.as_str(),
        "http://example.test/users",
        "the URL resolves {{base}} from the editor's scopes"
    );
    assert_eq!(
        sent[0].body,
        RequestBody::Bytes(br#"{"name":"Ada"}"#.to_vec()),
        "the body resolves {{name}} from the editor's scopes"
    );
    assert!(
        sent[0]
            .headers
            .iter()
            .chain(&sent[0].auto_headers)
            .any(|header| header.name == "Content-Type" && header.value == "application/json"),
        "the raw JSON body goes out with its JSON Content-Type"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().is_none())),
        "no warning bar shows when the editor's scopes resolve every name"
    );
    assert!(
        cx.debug_bounds(UNRESOLVED_WARNING_SELECTOR).is_none(),
        "the unresolved-variable bar is not on screen"
    );
}

#[gpui::test]
fn test_send_applies_the_auth_tab_credentials(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let (editor, cx) = open_editor(cx, &transport, plain_request());

    // Open the Auth tab the way a user does, pick Bearer and type its token — the
    // tab's own events write the auth into the draft the send is built from.
    let tab = cx
        .debug_bounds("request-tab-auth")
        .expect("the Auth tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);
    let auth_tab = cx.read(|cx| editor.read(cx).auth_tab().clone());
    auth_tab.update(cx, |tab, cx| tab.set_auth_type(AuthType::Bearer, cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let focus_handle = auth_tab
            .read(cx)
            .bearer_token_input()
            .read(cx)
            .focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input(LITERAL_TOKEN);
    cx.run_until_parked();

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "the send goes out exactly once");
    // The header is asserted, never printed (AGENTS.md › Security).
    assert!(
        sent[0].headers.iter().any(|header| {
            header.name == "Authorization" && header.value == format!("Bearer {LITERAL_TOKEN}")
        }),
        "the Auth tab's Bearer token goes out as the Authorization header"
    );
}

#[gpui::test]
fn test_every_finished_send_records_one_console_entry(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport, plain_request());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();

    assert_eq!(transport.requests().len(), 1, "the send went out");
    let status = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.status_line().map(|line| line.status)));
    assert_eq!(status, Some(200), "the response landed in the status line");
    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(
            entries.len(),
            1,
            "every finished send records exactly one Console entry"
        );
        let entry = entries.front().expect("the one entry the send recorded");
        assert_eq!(entry.method, Some(Method::Get));
        assert_eq!(entry.url, "https://example.com/ping");
        assert_eq!(entry.status, Some(200));
    });
}

#[gpui::test]
fn test_a_failed_send_reaches_the_console(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Connect {
            address: String::from("127.0.0.1:8080"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport, plain_request());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.inline_error().is_some())),
        "the failure still shows inline, under the URL bar"
    );
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some(),
        "the inline error is on screen"
    );
    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(
            entries.len(),
            1,
            "every failed send records exactly one Console entry"
        );
        let entry = entries.front().expect("the one entry the failure recorded");
        assert_eq!(entry.level, ConsoleLevel::Error);
    });
}

#[gpui::test]
fn test_scopes_survive_a_curl_paste(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let (editor, cx) = open_editor(cx, &transport, plain_request());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.set_scopes(scopes(), cx));
    });

    // The cURL paste replaces the whole request and rebuilds the fields under it.
    cx.write_to_clipboard(ClipboardItem::new_string(String::from(
        "curl '{{base}}/users'",
    )));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    // The rebuilt fields keep the editor's scopes: `{{base}}` still paints resolved.
    let resolved_style = cx.read(|cx| HighlightStyle {
        color: Some(cx.theme().colors().variable_resolved),
        ..HighlightStyle::default()
    });
    assert_eq!(
        url_highlights(&editor, cx),
        vec![(0..8, resolved_style)],
        "the rebuilt URL field still highlights {{base}} against the editor's scopes"
    );

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "the send goes out exactly once");
    assert_eq!(
        sent[0].url.as_str(),
        "http://example.test/users",
        "the pasted {{base}} resolves from the editor's scopes"
    );
}
