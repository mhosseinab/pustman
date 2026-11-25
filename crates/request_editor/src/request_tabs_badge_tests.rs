//! The request tab strip reads Params / Auth / Headers / Body / Settings, counts the
//! enabled Params and Headers rows, and marks Auth and Body with a dot while they are
//! set — the counts follow the user's edits.

use api_model::{Auth, HttpRequest, KeyValue};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;

fn row(key: &str, enabled: bool) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(String::from("v")),
        description: None,
        enabled,
        extra: Default::default(),
    }
}

fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.test/orders?status=open");
    request.params = vec![row("status", true)];
    request.headers = vec![
        row("Accept", true),
        row("X-Trace", true),
        row("X-Off", false),
    ];
    request.auth = Auth::Bearer {
        token: Some(String::from("{{token}}")),
        extra: Default::default(),
    };
    request
}

fn open(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    draw_window(cx);
    (editor, cx)
}

#[gpui::test]
fn test_tab_badges_count_enabled_rows_and_mark_set_auth(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, request());

    let badges = editor.read_with(cx, |editor, _| editor.tab_badges());
    assert_eq!(badges.params, Some(1), "one enabled query param");
    assert_eq!(
        badges.headers,
        Some(2),
        "the disabled header is not counted"
    );
    assert!(badges.auth_set, "a bearer token is set auth");
    assert!(!badges.body_set, "no body is not set");
    assert!(
        cx.debug_bounds("request-tab-headers-count").is_some(),
        "the Headers count paints in the tab"
    );
    assert!(
        cx.debug_bounds("request-tab-auth-indicator").is_some(),
        "the Auth tab paints its set dot"
    );
    assert!(cx.debug_bounds("request-tab-body-indicator").is_none());
}

#[gpui::test]
fn test_disabling_a_header_row_lowers_the_headers_count(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx, request());

    let table = editor.read_with(cx, |editor, cx| editor.headers().read(cx).table().clone());
    cx.update(|_, cx| {
        table.update(cx, |table, cx| {
            let first = table.row_ids()[0];
            table.set_enabled(first, false, cx);
        })
    });
    cx.run_until_parked();

    let badges = editor.read_with(cx, |editor, _| editor.tab_badges());
    assert_eq!(badges.headers, Some(1), "the count follows the edit");
}

#[gpui::test]
fn test_auth_sits_second_in_the_tab_strip(cx: &mut TestAppContext) {
    let (_editor, cx) = open(cx, request());
    let params = cx.debug_bounds("request-tab-params").expect("Params tab");
    let auth = cx.debug_bounds("request-tab-auth").expect("Auth tab");
    let headers = cx.debug_bounds("request-tab-headers").expect("Headers tab");
    assert!(params.left() < auth.left() && auth.left() < headers.left());
}
