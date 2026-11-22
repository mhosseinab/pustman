//! The pre-send check sees the keychain too (#198): a secret-kind variable with no
//! local value that [`SecretSources`] names a keychain entry for is resolved by the
//! send — `http_engine::prepare` fetches it before it resolves — so it must not raise
//! the unresolved bar, hold the send back or be recorded as unresolved. Driven the way
//! a user drives it: the real `request::Send` keystroke and the real bar's "Send
//! anyway" click, against a `secrets::MemorySecretStore` standing in for the OS
//! keychain under the same `SecretStore` contract. Every assertion is an outcome the
//! user can see — the names the bar lists, what reached the transport, and what the
//! Console recorded.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{HttpRequest, Variable, VariableKind};
use console::{ConsoleLevel, ConsoleStore};
use gpui::{AppContext, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use ui::test_support::{draw_window, init_test};
use variables::{ScopeKind, ScopeStack};

use super::{SEND_ANYWAY_SELECTOR, UNRESOLVED_WARNING_SELECTOR};
use crate::RequestEditor;
use crate::send::set_http_client;
use crate::variable_hover::SecretSources;

/// The workspace every fixture's keys and secrets are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The scope id the fixture's secret lives under (an environment's stable id in the
/// real wiring).
const SCOPE_ID: &str = "environment-a";

/// The fixture secret — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";

/// The scopes in effect: one environment holding the secret `token` with no local
/// value — the shape whose only value is the keychain's (REQ-VAR-SHARE-02).
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![Variable {
            key: String::from("token"),
            value: None,
            kind: VariableKind::Secret,
            enabled: true,
            description: None,
            extra: BTreeMap::new(),
        }],
    );
    scopes
}

/// The fixture's key names: an environment-scope secret under the test workspace.
fn fixture_key_of(kind: ScopeKind, name: &str) -> Option<SecretKey> {
    if kind == ScopeKind::Environment {
        SecretKey::new(WORKSPACE_ID, SCOPE_ID, name).ok()
    } else {
        None
    }
}

/// Where the fixture's secret lives: the test's keychain holding the obviously fake
/// value under the fixture key.
fn keychain_sources() -> SecretSources {
    let store = MemorySecretStore::new();
    let key = SecretKey::new(WORKSPACE_ID, SCOPE_ID, "token")
        .expect("the fixture's scope ids are valid keychain ids");
    store
        .set(&key, &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret stores");
    SecretSources {
        store: Arc::new(store),
        key_of: Arc::new(fixture_key_of),
    }
}

/// A draft whose URL names `{{token}}` — and `{{missing}}` when `extra` is named.
fn request(extra: Option<&str>) -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = match extra {
        Some(name) => format!("https://example.com/{{{{token}}}}/{{{{{name}}}}}"),
        None => String::from("https://example.com/{{token}}"),
    };
    request
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the Console installed so every send's
/// entry has somewhere to land. `sources` is what the send and the pre-send check read
/// the keychain through.
fn open_editor<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
    request: HttpRequest,
    sources: SecretSources,
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
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes(), cx);
            editor.set_secret_sources(sources, cx);
            editor.focus_url_bar(window, cx);
        });
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
fn test_a_keychain_secret_holds_no_bar_and_the_send_resolves_it(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport, request(None), keychain_sources());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let sent = transport.requests();
    assert_eq!(
        sent.len(),
        1,
        "a secret the keychain resolves holds no send back"
    );
    assert_eq!(
        sent.first().map(|request| request.url.as_str()),
        Some(format!("https://example.com/{FAKE_SECRET}").as_str()),
        "the send resolved the secret from the keychain"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().is_none())),
        "the bar names no variable the keychain resolves, got {:?}",
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().cloned()))
    );
    assert!(
        cx.debug_bounds(UNRESOLVED_WARNING_SELECTOR).is_none(),
        "no unresolved bar shows for a send that resolves"
    );
}

#[gpui::test]
fn test_the_bar_lists_only_the_names_no_keychain_secret_resolves(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_error(
        Duration::ZERO,
        TransportError::Connect {
            address: String::from("127.0.0.1:8080"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport, request(Some("missing")), keychain_sources());

    // The pre-send check holds the send back — but names only what nothing resolves.
    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        transport.requests().len(),
        0,
        "a name nothing resolves still holds the send back"
    );
    let warning = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().cloned()))
        .expect("the held-back send shows the warning bar");
    assert_eq!(
        warning.names(),
        &[String::from("missing")],
        "the bar lists only the names the keychain does not resolve"
    );

    // "Send anyway" sends — resolving `token` and leaving `missing` literal — and the
    // warned failure records only the names nothing resolved (REQ-VAR-07).
    let button = cx
        .debug_bounds(SEND_ANYWAY_SELECTOR)
        .expect("the warning bar's Send anyway button is on screen");
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(transport.requests().len(), 1, "the send went out");
    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert!(
            entries.iter().any(|entry| {
                entry.level == ConsoleLevel::Warning
                    && entry.raw_response == "Unresolved variables: missing"
            }),
            "the warned failure records the names nothing resolved (REQ-VAR-07), \
             got {entries:?}"
        );
    });
}

#[gpui::test]
fn test_a_secret_with_no_keychain_key_stays_on_the_bar(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let (editor, cx) = open_editor(cx, &transport, request(None), SecretSources::default());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        transport.requests().len(),
        0,
        "a secret no keychain key names holds the send back"
    );
    let warning = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().cloned()))
        .expect("the held-back send shows the warning bar");
    assert_eq!(
        warning.names(),
        &[String::from("token")],
        "a secret with no keychain entry is unresolved like any other name"
    );
}
