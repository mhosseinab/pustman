//! The Headers tab driven the way a user drives it: clicking the tab in the strip,
//! reading the auto headers the request's own rules list, and typing at them —
//! asserting what the user can see and what lands in the request draft (S27b).

use std::collections::BTreeMap;

use api_model::{Body, HttpRequest, KeyValue, Method, RawLanguage};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;

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

/// The auto headers and the user rows as the tab shows them: `(name, value)` pairs of
/// the auto rows, the table's rows, and the request draft's headers.
fn header_state(
    editor: &Entity<RequestEditor>,
    cx: &mut VisualTestContext,
) -> (Vec<(String, String)>, Vec<KeyValue>, Vec<KeyValue>) {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            let headers = editor.headers().read(cx);
            let auto = headers
                .auto_rows()
                .iter()
                .map(|row| (row.name.clone(), row.value.clone()))
                .collect();
            (auto, headers.rows(cx), editor.draft().headers.clone())
        })
    })
}

#[gpui::test]
fn auto_headers_listed_read_only(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request_with_json_body());
    open_headers_tab(cx);

    // The body's own Content-Type is listed, from the send pipeline's auto-header
    // rules (removing that source leaves this row missing and the test fails).
    let (auto, rows, draft_headers) = header_state(&editor, cx);
    assert!(
        auto.contains(&(
            String::from("Content-Type"),
            String::from("application/json")
        )),
        "the JSON body lists its Content-Type: {auto:?}"
    );

    // The row is on screen and badged as automatic.
    assert!(
        cx.debug_bounds("auto-row-Content-Type").is_some(),
        "the Content-Type auto row renders"
    );
    assert!(
        cx.debug_bounds("auto-badge-Content-Type").is_some(),
        "the Content-Type auto row carries the auto badge"
    );

    // It is not a user header: the editable table and the request draft hold none.
    assert!(
        rows.is_empty(),
        "the user rows hold no auto header: {rows:?}"
    );
    assert!(
        draft_headers.is_empty(),
        "the request headers hold no auto header: {draft_headers:?}"
    );

    // Typing at it changes nothing — the row is read-only and holds no text field.
    let row = cx
        .debug_bounds("auto-row-Content-Type")
        .expect("the Content-Type auto row renders");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_input("Content-Type: text/html");
    cx.run_until_parked();
    draw_window(cx);

    let (auto_after, rows_after, draft_after) = header_state(&editor, cx);
    assert_eq!(
        auto_after, auto,
        "typing at an auto row leaves the row unchanged"
    );
    assert!(
        rows_after.is_empty(),
        "typing at an auto row adds no user row: {rows_after:?}"
    );
    assert!(
        draft_after.is_empty(),
        "typing at an auto row never reaches the request headers: {draft_after:?}"
    );
}
