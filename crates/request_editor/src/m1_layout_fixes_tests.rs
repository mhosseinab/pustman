//! The M1 request-editor blockers and major layout bugs (S33n), driven the way a user
//! drives them and asserted on what paints: the raw body editor's painted size, the
//! Settings tab's number fields' painted width, the Headers table's checkbox label
//! painting nothing over the Key cell, and a failed or cancelled send leaving none of
//! the previous response's facts or panes behind. Every assertion is an outcome the
//! user can see on screen.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Body, HttpRequest, KeyValue};
use gpui::{
    AppContext, Entity, Focusable, Modifiers, OperatingSystem, TestAppContext, VisualTestContext,
    px, size,
};
use http_engine::{FakeTransport, HttpClient, TransportError};
use ui::components::toggle::toggle_label_selector;
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::body_tab::{BodyMode, RAW_EDITOR_MIN_HEIGHT_PX};
use crate::key_value_table::ENABLED_CHECKBOX_LABEL;
use crate::send::{STATUS_LINE_SELECTOR, set_http_client};

/// `response_viewer`'s body pane and error-panel test keys. That crate's selectors are
/// crate-private and this step may not edit it (see the report's `Decision:`), so the
/// keys are spelled out here — the `response_split_tests.rs` precedent.
const BODY_PANE_SELECTOR: &str = "response-body-pane";

fn error_kind_selector(kind: &str) -> String {
    format!("error-panel-kind-{kind}")
}

/// The draft the tests edit: a plain GET the URL bar starts out with.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request
}

/// One header row, as the Headers table seeds it from the request.
fn key_value(key: &str, value: Option<&str>) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: value.map(String::from),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// A request tab on the app's real init order and the real default keymap, sending
/// through a client backed by `transport`, with the cursor where a user starts: the URL
/// field (the `send_tests` shape — those helpers are module-private and their file is
/// read-only).
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

/// Whether anything paints under this debug key (`error_panel_tests.rs`'s pattern:
/// `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is leaked —
/// fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

#[gpui::test]
fn test_raw_body_editor_has_a_usable_height(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let (editor, cx) = open_editor(cx, &transport, request());
    cx.simulate_resize(size(px(1440.), px(900.)));
    draw_window(cx);
    click_control("request-tab-body", cx);

    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::Raw, cx));
    draw_window(cx);

    let area = cx.read(|cx| body_tab.read(cx).raw_text_area().clone());
    let bounds = cx
        .read(|cx| area.read(cx).painted_bounds())
        .expect("the raw body editor painted");
    assert!(
        bounds.size.height >= px(RAW_EDITOR_MIN_HEIGHT_PX),
        "the raw body editor paints at least its minimum height, not 0 px: {bounds:?}"
    );

    cx.update(|window, cx| {
        let focus_handle = area.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input("{\"a\":1}");
    cx.run_until_parked();
    draw_window(cx);

    let body = cx.read(|cx| editor.read(cx).draft().body.clone());
    match body {
        Body::Raw { text, .. } => {
            assert_eq!(text, "{\"a\":1}", "typing lands in the draft's raw body");
        }
        other => panic!("the draft carries the typed raw body: {other:?}"),
    }
}

#[gpui::test]
fn test_settings_number_inputs_fit_their_value(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let (_editor, cx) = open_editor(cx, &transport, request());
    click_control("request-tab-settings", cx);

    for field in ["settings-timeout-input", "settings-max-redirects-input"] {
        let bounds = cx
            .debug_bounds(field)
            .expect("the number field is on screen");
        assert!(
            bounds.size.width > px(40.),
            "{field} fits its value — more than 40 px at the default font size: {bounds:?}"
        );
    }
}

#[gpui::test]
fn test_header_toggle_label_is_not_painted_over_the_key(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    let mut request = request();
    request.headers = vec![key_value("Content-Type", Some("application/json"))];
    let (editor, cx) = open_editor(cx, &transport, request);
    click_control("request-tab-headers", cx);

    // The visible labels around it do paint — the hook that measures painted labels
    // works, so the assertion below is not vacuous.
    let auto_names: Vec<String> = cx.read(|cx| {
        editor
            .read(cx)
            .headers()
            .read(cx)
            .auto_rows()
            .iter()
            .map(|row| row.name.clone())
            .collect()
    });
    assert!(
        !auto_names.is_empty(),
        "the request carries auto headers, whose labels are visible"
    );
    assert!(
        shown(
            cx,
            &toggle_label_selector(&format!("Send {}", auto_names[0]))
        ),
        "a visible toggle label does paint — the hook that measures it works"
    );

    // The row's checkbox label paints nothing at all: it is the checkbox's accessible
    // name only (the "Enabled" column header is the row's visible meaning), so it can
    // never paint over the Key cell beside it.
    assert!(
        !shown(cx, &toggle_label_selector(ENABLED_CHECKBOX_LABEL)),
        "the row's checkbox label is its accessible name only — no text paints over the \
         Key cell"
    );
}

#[gpui::test]
fn test_failed_send_clears_the_previous_status(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::ZERO, 200, r#"{"ok":true}"#);
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: String::from("api.example.test"),
        },
    );
    let (editor, cx) = open_editor(cx, &transport, request());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        cx.read(|cx| editor.read(cx).status_line().map(|line| line.status)),
        Some(200),
        "the first send lands its 200"
    );

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let status = cx.read(|cx| editor.read(cx).status_line().map(|line| line.status));
    assert!(
        status.is_none(),
        "the failed send leaves no previous 200 behind: {status:?}"
    );
    assert!(
        shown(cx, &error_kind_selector("Dns")),
        "the response pane shows the error state"
    );
    assert!(
        !shown(cx, STATUS_LINE_SELECTOR),
        "no previous status strip sits next to the error state"
    );
}

#[gpui::test]
fn test_cancelled_send_clears_the_previous_status(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::ZERO, 200, r#"{"ok":true}"#);
    transport.push_response(Duration::from_secs(30), 200, "late");
    let (editor, cx) = open_editor(cx, &transport, request());

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        cx.read(|cx| editor.read(cx).status_line().map(|line| line.status)),
        Some(200),
        "the first send lands its 200"
    );

    cx.simulate_keystrokes(send_keys(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw_window(cx);

    let status = cx.read(|cx| editor.read(cx).status_line().map(|line| line.status));
    assert!(
        status.is_none(),
        "the cancelled send leaves no previous 200 behind: {status:?}"
    );
    assert!(
        shown(cx, &error_kind_selector("Cancelled")),
        "the response pane shows the cancelled state"
    );
    assert!(
        !shown(cx, STATUS_LINE_SELECTOR),
        "no stale 200 strip sits next to the cancelled state"
    );
    assert!(
        !shown(cx, BODY_PANE_SELECTOR),
        "no previous response body sits under the cancelled state"
    );
}
