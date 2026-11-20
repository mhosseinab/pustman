//! The Headers tab's autocomplete and auto-header switches driven the way a user drives
//! them (REQ-HTTP-03, S27d): typing into a header value cell offers that header's common
//! values, and each auto row's switch leaves that header out of the send.

use std::collections::BTreeMap;

use api_model::{Body, HttpRequest, KeyValue, Method, RawLanguage, RequestSettings};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::key_value_table::RowField;

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

/// A draft POST with a JSON body — the body mode that describes itself with a
/// `Content-Type`.
fn request_with_json_body() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request.url = String::from("https://example.com/items");
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from("{\"name\":\"pustman\"}"),
        extra: BTreeMap::new(),
    };
    request
}

/// A draft with one header row per name under test.
fn request_with_header_rows() -> HttpRequest {
    let mut request = request_with_json_body();
    request.headers = vec![key_value("Content-Type", None), key_value("Accept", None)];
    request
}

/// A request tab on the app's real init order.
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
    draw_window(cx);
    (editor, cx)
}

/// Open the Headers tab the way a user does: click it in the request tab strip.
fn open_headers_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-headers")
        .expect("the Headers tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The user rows as the tab shows them.
fn rows(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> Vec<KeyValue> {
    cx.read(|cx| editor.read_with(cx, |editor, cx| editor.headers().read(cx).rows(cx)))
}

/// The table's stable row ids, in order.
fn row_ids(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> Vec<u64> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor.headers().read(cx).table().read(cx).row_ids()
        })
    })
}

/// Put the cursor in one row's value cell — the path a click on the cell takes.
fn focus_value_cell(editor: &Entity<RequestEditor>, row_id: u64, cx: &mut VisualTestContext) {
    let table =
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.headers().read(cx).table().clone()));
    cx.update(|window, cx| {
        table.update(cx, |table, cx| {
            table.focus_cell(row_id, RowField::Value, window, cx)
        });
    });
    draw_window(cx);
}

/// The draft's request settings as the Headers tab edits them (REQ-HTTP-03).
fn draft_settings(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> RequestSettings {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor.headers().read(cx).draft_settings().clone()
        })
    })
}

/// Click one auto row's send switch, the way a user does.
fn click_auto_toggle(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(selector)
        .expect("the auto row's switch renders");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn value_suggestions_follow_header_name(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request_with_header_rows());
    open_headers_tab(cx);
    let ids = row_ids(&editor, cx);
    assert_eq!(ids.len(), 2, "the table opens on the request's two rows");

    // In a Content-Type row's value cell the suggestions are Content-Type's values.
    focus_value_cell(&editor, ids[0], cx);
    cx.simulate_input("app");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds("suggestion-application/x-www-form-urlencoded")
            .is_some(),
        "a Content-Type row suggests Content-Type values"
    );
    assert!(
        cx.debug_bounds("suggestion-*/*").is_none(),
        "an Accept value never appears while the row is a Content-Type row"
    );

    // Enter accepts the selected suggestion into the value cell.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
    let rows = rows(&editor, cx);
    assert_eq!(
        rows[0].value.as_deref(),
        Some("application/json"),
        "Enter puts the selected suggestion into the value cell"
    );

    // In an Accept row the suggestions are Accept's values: the list follows the row's
    // header name. (The ids are re-read: accepting rewrites the rows wholesale.)
    let ids = row_ids(&editor, cx);
    focus_value_cell(&editor, ids[1], cx);
    cx.simulate_input("*");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds("suggestion-*/*").is_some(),
        "an Accept row suggests Accept's values"
    );
    assert!(
        cx.debug_bounds("suggestion-application/x-www-form-urlencoded")
            .is_none(),
        "Content-Type values are gone once the row is an Accept row"
    );
}

#[gpui::test]
fn auto_header_toggle_updates_draft_settings(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request_with_json_body());
    open_headers_tab(cx);
    assert!(
        draft_settings(&editor, cx).disabled_auto_headers.is_empty(),
        "every auto header starts sent"
    );

    // Switch User-Agent off at its row's control.
    click_auto_toggle("auto-toggle-User-Agent", cx);
    let settings = draft_settings(&editor, cx);
    assert_eq!(
        settings.disabled_auto_headers,
        vec![String::from("User-Agent")],
        "switching the row off must leave User-Agent out of the send (REQ-HTTP-03)"
    );

    // And switching it back on sends it again.
    click_auto_toggle("auto-toggle-User-Agent", cx);
    assert!(
        draft_settings(&editor, cx).disabled_auto_headers.is_empty(),
        "switching the row back on clears the setting again"
    );
}
