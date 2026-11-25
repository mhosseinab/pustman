//! The failed-request row (S48u6, mockup section 9): a send that fails records its
//! request with the error, and the row expands to the error message and a "Request
//! headers" list whose credential values read "redacted" — never the credential.

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, ItemId, KeyValue, Method};
use gpui::{Entity, TestAppContext, VisualTestContext};
use http_engine::{PipelineError, TransportError};
use ui::test_support::{draw_window, init_test};

use super::{ConsolePanel, ERROR_MESSAGE_SELECTOR, REQUEST_HEADERS_SELECTOR};
use crate::{ConsoleLevel, ConsoleStore, record_error};

/// An obviously fake credential: it must appear in no drawn text.
const FAKE_TOKEN: &str = "token-REDACTED-example";

fn header(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: Default::default(),
    }
}

fn failing_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "Users".to_string(),
        method: Method::Get,
        url: "http://127.0.0.1:9/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![
            header("Accept", "*/*"),
            header("Authorization", &format!("Bearer {FAKE_TOKEN}")),
        ],
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// A console with one failed send recorded through the feed and its row expanded.
fn failed_and_expanded(cx: &mut TestAppContext) -> (Entity<ConsolePanel>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    let error = PipelineError::Transport(TransportError::Connect {
        address: "127.0.0.1:9".to_string(),
    });
    cx.update(|_, cx| record_error(Some(&failing_request()), &error, cx));
    panel.update(cx, |panel, cx| panel.toggle_expanded(0, cx));
    draw_window(cx);
    (panel, cx)
}

#[gpui::test]
async fn test_a_failed_request_expands_to_its_message_and_redacted_request_headers(
    cx: &mut TestAppContext,
) {
    let (panel, cx) = failed_and_expanded(cx);

    cx.read(|cx| {
        let entry = cx
            .global::<ConsoleStore>()
            .entries()
            .front()
            .cloned()
            .expect("the failure recorded one entry");
        assert_eq!(entry.level, ConsoleLevel::Error);
        assert!(
            !entry.raw_request.contains(FAKE_TOKEN),
            "the recorded request never holds the credential"
        );
    });
    assert!(
        cx.debug_bounds(ERROR_MESSAGE_SELECTOR).is_some(),
        "the expanded row shows the error message"
    );
    assert!(
        cx.debug_bounds(REQUEST_HEADERS_SELECTOR).is_none(),
        "the headers start folded"
    );

    panel.update(cx, |panel, cx| panel.toggle_request_headers(cx));
    draw_window(cx);

    assert!(
        cx.debug_bounds(REQUEST_HEADERS_SELECTOR).is_some(),
        "the Request headers list opens"
    );
    let texts = cx.read(|cx| {
        panel
            .read(cx)
            .expanded_error_texts(cx)
            .expect("the expanded entry is a failed request")
    });
    assert!(texts.contains(&"Request headers".to_string()));
    assert!(texts.contains(&"Accept: */*".to_string()));
    assert!(
        texts.contains(&"Authorization: redacted".to_string()),
        "Authorization reads redacted: {texts:?}"
    );
    assert!(
        texts.contains(&"Host: 127.0.0.1:9".to_string()),
        "the host the request went to is listed: {texts:?}"
    );
    assert!(
        texts.iter().all(|text| !text.contains(FAKE_TOKEN)),
        "the credential appears in no drawn text"
    );
}

#[gpui::test]
async fn test_a_failure_with_no_request_stays_a_notice(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    let error = PipelineError::Transport(TransportError::Connect {
        address: "127.0.0.1:9".to_string(),
    });
    cx.update(|_, cx| record_error(None, &error, cx));
    panel.update(cx, |panel, cx| panel.toggle_expanded(0, cx));
    draw_window(cx);

    assert!(
        cx.read(|cx| panel.read(cx).expanded_error_texts(cx))
            .is_none(),
        "a failure with no request has no request headers to list"
    );
}
