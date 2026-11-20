//! cURL paste and Copy as cURL driven the way a user drives them (S27c): the real
//! `text_input::Paste` keystroke into the focused URL field writes the clipboard, and
//! the real `request::CopyAsCurl` action fills it — asserting what the user sees after
//! (URL bar, tables, dirty tab, the one-step undo) and what lands on the clipboard.
//! The step mandates exactly these two tests, so the import's own states — the undo and
//! the inline error a command that does not parse raises — are phases of the paste test.

use api_model::{HttpRequest, KeyValue, Method};
use gpui::{ClipboardItem, Entity, OperatingSystem, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Item;

use super::UNDO_IMPORT_SELECTOR;
use crate::RequestEditor;
use crate::send::INLINE_ERROR_SELECTOR;

/// The cURL command the import test pastes: a POST with a query, a header and a JSON
/// body — everything the URL bar, Params and Headers show afterwards.
const CURL_COMMAND: &str = "curl -X POST 'https://api.example.com/users?role=admin' -H 'Content-Type: application/json' -d '{\"name\":\"Ada\"}'";

/// The request the tab starts with. It is kept whole by the import, so the undo has
/// something the test can see restored.
fn starting_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/keep?draft=1");
    request.headers = vec![key_value("X-Draft", "before")];
    request
}

fn key_value(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: Some(String::from(value)),
        description: None,
        enabled: true,
        extra: Default::default(),
    }
}

/// A request tab on the app's real init order and the real default keymap, with the
/// cursor where a paste happens: the URL field.
fn open_editor(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
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

/// The `text_input::Paste` keystroke on this platform — Cmd on macOS, Ctrl elsewhere,
/// exactly as `text_input::init` binds it.
fn paste_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

#[gpui::test]
fn pasting_curl_imports_request(cx: &mut TestAppContext) {
    let request = starting_request();
    let (editor, cx) = open_editor(cx, request.clone());

    cx.write_to_clipboard(ClipboardItem::new_string(CURL_COMMAND.to_string()));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    // The URL bar shows the imported method and URL.
    let (method, url) = cx.read(|cx| {
        let bar = editor.read(cx).url_bar().read(cx);
        (bar.method().clone(), bar.url(cx))
    });
    assert_eq!(method, Method::Post, "the method is the command's -X");
    assert_eq!(
        url, "https://api.example.com/users?role=admin",
        "the URL field holds the command's URL, not the command"
    );

    // The tables show its rows, and the tab is dirty because the draft changed.
    let (params, headers, dirty) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.params().read(cx).query_rows(cx),
                editor.headers().read(cx).rows(cx),
                editor.is_dirty(cx),
            )
        })
    });
    assert!(
        params
            .iter()
            .any(|row| row.key == "role" && row.value.as_deref() == Some("admin")),
        "the Params table shows the imported query: {params:?}"
    );
    assert!(
        headers.iter().any(|row| {
            row.key == "Content-Type" && row.value.as_deref() == Some("application/json")
        }),
        "the Headers table shows the imported header: {headers:?}"
    );
    assert!(dirty, "the import leaves the tab dirty");

    // The import is one undoable change: the inline row offers it, Tab reaches the
    // button (it is in the tab order after the URL field) and Enter runs it.
    assert!(
        cx.debug_bounds(UNDO_IMPORT_SELECTOR).is_some(),
        "an inline Undo import row is offered under the URL bar"
    );
    cx.update(|window, cx| window.focus_next(cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    let (restored, url, dirty) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.draft().clone(),
                editor.url_bar().read(cx).url(cx),
                editor.is_dirty(cx),
            )
        })
    });
    assert_eq!(
        restored, request,
        "one Undo restores the whole request the import replaced"
    );
    assert_eq!(url, request.url, "URL bar and all");
    assert!(!dirty, "the tab is clean again — undoing is not an edit");
    assert!(
        cx.debug_bounds(UNDO_IMPORT_SELECTOR).is_none(),
        "and there is no import left to undo"
    );

    // A cURL command that does not parse imports nothing and says why inline, under
    // the URL bar — and the URL field keeps what it had.
    cx.write_to_clipboard(ClipboardItem::new_string(
        "curl --wat https://example.com".to_string(),
    ));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let (error, url, dirty) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.inline_error().map(|error| error.message.clone()),
                editor.url_bar().read(cx).url(cx),
                editor.is_dirty(cx),
            )
        })
    });
    let error = error.expect("a command that does not parse reports why inline");
    assert!(
        error.contains("unsupported cURL flag"),
        "the inline error carries the parser's own text: {error:?}"
    );
    assert!(
        cx.debug_bounds(INLINE_ERROR_SELECTOR).is_some(),
        "and it renders under the URL bar"
    );
    assert_eq!(
        url, request.url,
        "the URL field is left unchanged by a failed import"
    );
    assert!(!dirty, "and the tab still holds the request it had");
}

#[gpui::test]
fn copy_as_curl_writes_clipboard(cx: &mut TestAppContext) {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request.url = String::from("https://example.com/items");
    request.headers = vec![key_value("Content-Type", "application/json")];
    let (_editor, cx) = open_editor(cx, request);

    cx.dispatch_action(pustman_actions::request::CopyAsCurl);
    cx.run_until_parked();

    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("Copy as cURL puts a command on the clipboard");
    assert!(
        copied.starts_with("curl -X POST "),
        "the command names the request's method: {copied:?}"
    );
    assert!(
        copied.contains("'https://example.com/items'"),
        "and its URL: {copied:?}"
    );
    assert!(
        copied.contains("-H 'Content-Type: application/json'"),
        "and its headers: {copied:?}"
    );
}
