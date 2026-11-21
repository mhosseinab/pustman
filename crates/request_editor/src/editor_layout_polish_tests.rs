//! The request editor's layout reads as one workbench: Send ends the URL row instead of
//! sitting alone under it, the content is inset from the pane edge, and the Settings tab
//! is an aligned form — every control in one column, every "use default" box in another.

use std::sync::Arc;

use gpui::{AppContext, Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, px};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::send::{SEND_BUTTON_SELECTOR, STATUS_LINE_SELECTOR, set_http_client};
use crate::url_bar::{METHOD_CONTROL_SELECTOR, URL_FIELD_SELECTOR};

/// The smallest inset that reads as padding rather than as touching the edge.
const MIN_INSET: Pixels = px(12.);

fn open_editor(cx: &mut TestAppContext) -> &mut VisualTestContext {
    let transport = FakeTransport::new(cx.executor());
    open_editor_with(cx, &transport)
}

fn open_editor_with<'a>(
    cx: &'a mut TestAppContext,
    transport: &FakeTransport,
) -> &'a mut VisualTestContext {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        set_http_client(client, cx);
    });
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    let (_editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    draw_window(cx);
    cx
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

#[gpui::test]
fn test_send_ends_the_url_row(cx: &mut TestAppContext) {
    let cx = open_editor(cx);
    let field = bounds(URL_FIELD_SELECTOR, cx);
    let send = bounds(SEND_BUTTON_SELECTOR, cx);

    assert_eq!(
        send.origin.y, field.origin.y,
        "Send shares the URL field's row, not a row of its own"
    );
    assert!(
        send.left() >= field.right(),
        "Send sits after the URL field: send.left {:?}, field.right {:?}",
        send.left(),
        field.right()
    );
}

#[gpui::test]
fn test_request_content_is_inset_from_the_pane_edge(cx: &mut TestAppContext) {
    let cx = open_editor(cx);
    let method = bounds(METHOD_CONTROL_SELECTOR, cx);
    let content = bounds(crate::REQUEST_TAB_CONTENT_SELECTOR, cx);

    assert!(
        method.left() >= MIN_INSET && method.top() >= MIN_INSET,
        "the URL row is inset from the pane's edge: {method:?}"
    );
    assert!(
        content.left() >= MIN_INSET,
        "the tab content is inset from the pane's edge: {content:?}"
    );
}

#[gpui::test]
fn test_settings_controls_and_default_boxes_line_up_in_columns(cx: &mut TestAppContext) {
    let cx = open_editor(cx);
    let tab = bounds("request-tab-settings", cx);
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let controls = [
        "settings-timeout-input",
        "settings-follow-redirects-switch",
        "settings-max-redirects-input",
        "settings-follow-original-method-switch",
        "settings-verify-tls-switch",
        "settings-encode-url-switch",
        "settings-remove-referer-switch",
        "settings-http-version-control",
    ];
    let defaults = [
        "settings-timeout-default",
        "settings-follow-redirects-default",
        "settings-max-redirects-default",
        "settings-follow-original-method-default",
        "settings-verify-tls-default",
        "settings-encode-url-default",
        "settings-remove-referer-default",
        "settings-http-version-default",
    ];

    let control_left = bounds(controls[0], cx).left();
    for selector in controls {
        assert_eq!(
            bounds(selector, cx).left(),
            control_left,
            "{selector} starts in the shared control column"
        );
    }
    let default_left = bounds(defaults[0], cx).left();
    for selector in defaults {
        assert_eq!(
            bounds(selector, cx).left(),
            default_left,
            "{selector} starts in the shared default column"
        );
    }
    assert!(
        default_left > control_left,
        "the default column follows the control column"
    );
}

#[gpui::test]
fn test_response_is_inset_from_the_pane_edge(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let cx = open_editor_with(cx, &transport);
    let send = bounds(SEND_BUTTON_SELECTOR, cx);
    cx.simulate_click(send.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let status_line = bounds(STATUS_LINE_SELECTOR, cx);
    assert!(
        status_line.left() >= MIN_INSET,
        "the response status line is inset like the request above it: {status_line:?}"
    );
}
