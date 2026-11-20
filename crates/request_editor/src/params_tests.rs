//! The Params tab driven the way a user drives it: typing into the URL bar and into the
//! table cells, asserting what the user can see — the rows, the URL text, and the
//! request draft behind them. Every test name is a behavior sentence (S27a).

use std::collections::BTreeMap;

use api_model::{HttpRequest, KeyValue};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::Params;
use crate::RequestEditor;
use crate::key_value_table::RowField;

/// Which of the tab's two tables a test drives.
#[derive(Clone, Copy)]
enum Table {
    Query,
    Path,
}

/// A draft GET to `url`, the way a request tab opens.
fn request(url: &str) -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from(url);
    request
}

/// A row with no description and nothing imported in `extra`.
fn key_value(key: &str, value: Option<&str>) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: value.map(String::from),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// A request tab on the app's real init order, with the cursor where a user starts: the
/// URL field.
fn open_editor(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    draw_window(cx);
    (editor, cx)
}

/// The platform's select-all keystroke.
fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

/// Replace the URL field's text the way a user does: select all, then type.
fn type_url(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext, url: &str) {
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    cx.simulate_keystrokes(select_all_keystroke());
    cx.simulate_input(url);
    cx.run_until_parked();
    draw_window(cx);
}

/// Type `text` at the end of the URL field — the edit a user makes to add a segment
/// without retyping what is already there.
fn append_to_url(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    cx.simulate_keystrokes("end");
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// Put the cursor in one table cell — the path a click on the cell takes.
fn focus_cell(
    editor: &Entity<RequestEditor>,
    table: Table,
    row_id: u64,
    field: RowField,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let params = editor.params().clone();
            params.update(cx, |params: &mut Params, cx| {
                let table = match table {
                    Table::Query => params.query_table().clone(),
                    Table::Path => params.path_table().clone(),
                };
                table.update(cx, |table, cx| table.focus_cell(row_id, field, window, cx));
            });
        });
    });
    draw_window(cx);
}

/// The URL text, the query rows and their stable ids — everything the tab shows.
fn query_state(
    editor: &Entity<RequestEditor>,
    cx: &mut VisualTestContext,
) -> (String, Vec<KeyValue>, Vec<u64>) {
    cx.read(|cx| {
        let url = editor.read_with(cx, |editor, cx| editor.url_bar().read(cx).url(cx));
        let (rows, ids) = editor.read_with(cx, |editor, cx| {
            let table = editor.params().read(cx).query_table().read(cx);
            (table.rows(), table.row_ids())
        });
        (url, rows, ids)
    })
}

/// The path-variable rows and their stable ids.
fn path_state(
    editor: &Entity<RequestEditor>,
    cx: &mut VisualTestContext,
) -> (Vec<KeyValue>, Vec<u64>) {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            let table = editor.params().read(cx).path_table().read(cx);
            (table.rows(), table.row_ids())
        })
    })
}

/// The draft's params and path variables.
fn draft_rows(
    editor: &Entity<RequestEditor>,
    cx: &mut VisualTestContext,
) -> (Vec<KeyValue>, Vec<KeyValue>) {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            (
                editor.draft().params.clone(),
                editor.draft().path_variables.clone(),
            )
        })
    })
}

#[gpui::test]
fn editing_url_updates_params_table(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request("https://example.com/items"));
    let (_, rows, _) = query_state(&editor, cx);
    assert!(rows.is_empty(), "a URL with no query has no rows: {rows:?}");

    type_url(
        &editor,
        cx,
        "https://example.com/items?limit=10&sort={{mode}}",
    );

    let (url, rows, _) = query_state(&editor, cx);
    assert_eq!(
        url, "https://example.com/items?limit=10&sort={{mode}}",
        "the URL keeps {{mode}} verbatim — no parsing, no percent-decoding"
    );
    assert_eq!(
        rows,
        vec![
            key_value("limit", Some("10")),
            key_value("sort", Some("{{mode}}"))
        ],
        "every query parameter becomes one row"
    );
    let (draft_params, _) = draft_rows(&editor, cx);
    assert_eq!(draft_params, rows, "the request draft follows the table");
}

#[gpui::test]
fn editing_params_updates_url(cx: &mut TestAppContext) {
    let mut request = request("https://example.com/search");
    let mut edited = key_value("q", Some("1"));
    edited.description = Some(String::from("what to find"));
    edited.extra = BTreeMap::from([(String::from("x-imported"), serde_json::json!("keep me"))]);
    let mut disabled = key_value("skip", Some("9"));
    disabled.enabled = false;
    request.params = vec![edited, disabled];
    let (editor, cx) = open_editor(cx, request);
    let (_, rows, ids_before) = query_state(&editor, cx);
    assert_eq!(rows.len(), 2, "the table opens on the request's params");

    focus_cell(&editor, Table::Query, ids_before[0], RowField::Value, cx);
    cx.simulate_input("23");
    cx.run_until_parked();
    draw_window(cx);

    let (url, rows, ids_after) = query_state(&editor, cx);
    assert_eq!(
        url, "https://example.com/search?q=123",
        "the edited row rebuilds the URL and the disabled row is omitted from it"
    );
    assert_eq!(
        ids_after, ids_before,
        "the round trip through the URL must not rebuild the table (no feedback loop)"
    );
    assert_eq!(rows.len(), 2, "the disabled row is kept in the table");
    assert_eq!(
        rows[0].value.as_deref(),
        Some("123"),
        "the cell edit landed"
    );
    assert_eq!(
        rows[0].extra.get("x-imported"),
        Some(&serde_json::json!("keep me")),
        "a rewritten row keeps its imported extra fields"
    );
    assert!(!rows[1].enabled, "the disabled row stays disabled");
    assert_eq!(rows[1].key, "skip", "the disabled row stays itself");
    let (draft_params, _) = draft_rows(&editor, cx);
    assert_eq!(draft_params, rows, "the request draft follows the table");
}

#[gpui::test]
fn path_variable_rows_follow_url(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request("https://example.com/users/:id"));
    let (rows, ids) = path_state(&editor, cx);
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        vec!["id"],
        "a :name segment becomes a path-variable row"
    );

    focus_cell(&editor, Table::Path, ids[0], RowField::Value, cx);
    cx.simulate_input("42");
    cx.run_until_parked();
    draw_window(cx);

    let (rows, ids_after) = path_state(&editor, cx);
    assert_eq!(rows[0].value.as_deref(), Some("42"), "the value landed");
    assert_eq!(ids_after, ids, "editing a value does not rebuild the rows");
    let (url, _, _) = query_state(&editor, cx);
    assert_eq!(
        url, "https://example.com/users/:id",
        "editing a path-variable value does not edit the URL"
    );

    // A new `:name` segment — added the way a user adds one, without retyping the
    // segments that are already there (a name that stays keeps its value).
    append_to_url(&editor, cx, "/posts/:post_id");
    let (rows, _) = path_state(&editor, cx);
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        vec!["id", "post_id"],
        "a new :name segment adds a row"
    );
    assert_eq!(
        rows[0].value.as_deref(),
        Some("42"),
        "a row whose name stays keeps its value"
    );

    type_url(&editor, cx, "https://example.com/users/:slug");
    let (rows, _) = path_state(&editor, cx);
    assert_eq!(
        rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(),
        vec!["slug"],
        "a :name segment that disappears takes its row with it"
    );
    let (_, draft_path) = draft_rows(&editor, cx);
    assert_eq!(draft_path, rows, "the request draft follows the table");
}
