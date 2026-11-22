//! The cURL import's field rebuild (#159): `curl_paste`'s `replace_request` and
//! [`RequestEditor::new`] build the request-derived children through one shared
//! construction path, so a rebuild must leave every rebuilt field wired back to the
//! editor — an edit in the rebuilt URL bar, Params table or Headers table has to reach
//! the draft exactly as the original construction's does, and the same after the
//! rebuild "Undo import" runs. Driven the way a user drives it: the real paste
//! keystroke, a real keystroke in the URL field and real table edits.

use api_model::{HttpRequest, KeyValue, Method};
use gpui::{ClipboardItem, Entity, OperatingSystem, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::UNDO_IMPORT_SELECTOR;
use crate::RequestEditor;

/// The cURL command the tests paste: a POST with a query and a header — rows the
/// rebuilt Params and Headers tables hold.
const CURL_COMMAND: &str = "curl -X POST 'https://api.example.com/users?role=admin' -H 'Content-Type: application/json' -d '{\"name\":\"Ada\"}'";

/// The request the tab starts with: one query row and one header row, so the tables
/// rebuilt by an import and by its undo both hold a row to edit.
fn starting_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/keep?draft=1");
    request.params = vec![key_value("draft", "1")];
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

/// Import the cURL command the way a user does: paste it into the focused URL field.
fn import_curl(cx: &mut VisualTestContext) {
    cx.write_to_clipboard(ClipboardItem::new_string(CURL_COMMAND.to_string()));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
}

/// Take the import back the way a user does: the Undo button is the next tab stop
/// after the URL field, and Enter runs it.
fn undo_import(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus_next(cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
}

/// Uncheck the first row's enabled box in the rebuilt Headers table (`headers`) or the
/// rebuilt Params query table — a table edit that must reach the draft through the
/// rebuilt tab's subscription.
fn uncheck_first_row(cx: &mut VisualTestContext, editor: &Entity<RequestEditor>, headers: bool) {
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            let table = if headers {
                editor.headers().read(cx).table().clone()
            } else {
                editor.params().read(cx).query_table().clone()
            };
            table.update(cx, |table, cx| {
                let id = table
                    .row_ids()
                    .first()
                    .copied()
                    .expect("both tables hold the rows their request came with");
                table.set_enabled(id, false, cx);
            });
        });
    });
    cx.run_until_parked();
}

#[gpui::test]
fn a_curl_imports_rebuilt_fields_keep_wiring_their_edits_to_the_draft(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, starting_request());
    import_curl(cx);
    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().method.clone())),
        Method::Post,
        "the import landed through the rebuilt URL bar"
    );

    // The rebuilt URL bar still drives the draft.
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.ends_with("/v2"))),
        "an edit in the rebuilt URL field reaches the draft"
    );

    // The rebuilt Headers table still drives the draft.
    uncheck_first_row(cx, &editor, true);
    assert!(
        cx.read(|cx| {
            editor.read_with(cx, |editor, _| {
                editor
                    .draft()
                    .headers
                    .iter()
                    .any(|row| row.key == "Content-Type" && !row.enabled)
            })
        }),
        "an edit in the rebuilt Headers table reaches the draft"
    );

    // The rebuilt Params table still drives the draft.
    uncheck_first_row(cx, &editor, false);
    assert!(
        cx.read(|cx| {
            editor.read_with(cx, |editor, _| {
                editor
                    .draft()
                    .params
                    .iter()
                    .any(|row| row.key == "role" && !row.enabled)
            })
        }),
        "an edit in the rebuilt Params table reaches the draft"
    );
}

#[gpui::test]
fn undo_imports_rebuilt_fields_keep_wiring_their_edits_to_the_draft(cx: &mut TestAppContext) {
    let request = starting_request();
    let (editor, cx) = open_editor(cx, request.clone());
    import_curl(cx);
    undo_import(cx);
    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().clone())),
        request,
        "the undo rebuilt the request it restored"
    );
    assert!(
        cx.debug_bounds(UNDO_IMPORT_SELECTOR).is_none(),
        "and there is no import left to undo"
    );

    // The rebuild the undo ran is wired like the import's: an edit reaches the draft.
    uncheck_first_row(cx, &editor, true);
    assert!(
        cx.read(|cx| {
            editor.read_with(cx, |editor, _| {
                editor
                    .draft()
                    .headers
                    .iter()
                    .any(|row| row.key == "X-Draft" && !row.enabled)
            })
        }),
        "an edit in the restored Headers table reaches the draft"
    );
}
