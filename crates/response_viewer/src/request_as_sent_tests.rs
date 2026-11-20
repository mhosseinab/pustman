//! The "Request as sent" record (REQ-NET-04): the section lists what the send put on the
//! wire — the method, the final URL, every header including the auto-generated and the
//! jar's `Cookie` — and every value it shows is masked through `api_model::redact`, so a
//! secret the send substituted or an `Authorization` credential never reaches the screen
//! (PRD §7). A row dropped hides a header the user's own send carried; a row shown raw
//! leaks it.

use std::collections::BTreeMap;
use std::time::Duration;

use api_model::{
    Auth, Body, HttpRequest, ItemId, KeyValue, MASK, Method, RawLanguage, SecretValues,
};
use gpui::TestAppContext;
use http_engine::{
    CookieJar, Header, Prepared, RedirectHop, RequestBody, ResolvedRequest, Response, ResponseBody,
    SendSettings, prepare,
};
use secrets::MemorySecretStore;
use ui::test_support::{draw_window, init_test};
use url::Url;
use variables::{DynamicValues, ScopeStack};

use super::RequestAsSentRow;
use crate::timeline::TimelineTab;

#[gpui::test]
fn request_as_sent_includes_auto_and_cookie_headers(cx: &mut TestAppContext) {
    cx.update(init_test);
    // The request the send prepared: one user header and a JSON body whose
    // `Content-Type` the body mode generates (REQ-HTTP-03).
    let prepared = prepared_request();
    // The `Cookie` header the jar would apply to this send — the caller's input.
    let cookie = jar_cookie("https://example.test/items");
    // The response the redirect chain ended at: the final URL the section shows is the
    // Location the last hop was followed to, not the URL the request went to.
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.redirects = vec![RedirectHop {
        method: Method::Get,
        url: Url::parse("https://example.test/items").expect("the fixture URL must parse"),
        status: 302,
        headers: Vec::new(),
        location: Some(
            Url::parse("https://example.test/final").expect("the fixture Location must parse"),
        ),
        timing: Some(Duration::from_millis(12)),
    }];

    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| {
        view.set_request_as_sent(
            &prepared.resolved,
            &prepared.secrets_used,
            Some(cookie),
            &response,
            cx,
        )
    });
    draw_window(cx);

    let rows = view.read_with(cx, |view, _| view.request_as_sent_rows().to_vec());

    let method = row(&rows, "Method").expect("the method row is listed");
    assert_eq!(
        method.value.to_string(),
        "POST",
        "the method the request went out with reaches the section"
    );
    let final_url = row(&rows, "Final URL").expect("the final URL row is listed");
    assert_eq!(
        final_url.value.to_string(),
        "https://example.test/final",
        "the final URL is the Location the last redirect hop was followed to"
    );

    let trace = row(&rows, "X-Trace").expect("the user header is listed");
    assert_eq!(
        trace.value.to_string(),
        "trace-1",
        "the user header's value reaches the row"
    );
    assert!(!trace.auto, "a header the user wrote is not marked auto");

    let content_type = row(&rows, "Content-Type").expect("the body's auto Content-Type is listed");
    assert!(
        content_type.auto,
        "a header the body mode generated is marked auto"
    );

    let cookie = row(&rows, "Cookie").expect("the jar's Cookie header is listed");
    assert_eq!(
        cookie.value.to_string(),
        MASK,
        "the Cookie value is masked whatever the secrets are (PRD §7)"
    );
}

#[gpui::test]
fn request_as_sent_masks_secrets_and_authorization(cx: &mut TestAppContext) {
    cx.update(init_test);
    // Both fixtures are obviously fake: the query and the header carry the send's
    // secret, the Authorization header a plain credential.
    let secret = "token-REDACTED-example";
    let request = ResolvedRequest::new(
        Method::Get,
        "https://example.test/items?token=token-REDACTED-example",
        vec![
            Header::new("X-Api-Key", secret),
            Header::new("Authorization", "Bearer abc"),
        ],
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("the fixture request must resolve");
    let secrets = SecretValues::from_values([secret]);
    let response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));

    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| {
        view.set_request_as_sent(&request, &secrets, None, &response, cx)
    });
    draw_window(cx);

    let rows = view.read_with(cx, |view, _| view.request_as_sent_rows().to_vec());
    let shown = rows
        .iter()
        .map(|row| format!("{} {}", row.label, row.value))
        .collect::<Vec<String>>()
        .join("\n");

    assert!(
        !shown.contains(secret),
        "no row keeps the secret the send substituted (PRD §7)"
    );
    assert!(
        !shown.contains("abc"),
        "no row keeps the Authorization credential"
    );
    assert!(
        shown.contains(MASK),
        "a masked value shows the mask, so the user sees something was hidden"
    );

    let authorization = row(&rows, "Authorization").expect("the Authorization header is listed");
    assert_eq!(
        authorization.value.to_string(),
        MASK,
        "a credential header is masked in full whatever the secrets are (REQ-HIST-03)"
    );
    let final_url = row(&rows, "Final URL").expect("the final URL row is listed");
    assert!(
        final_url.value.contains(MASK),
        "the secret in the URL query is masked in the final URL row"
    );
}

/// The request as `prepare` resolves it (REQ-HTTP-03): one user header and a JSON body,
/// derived through `http_engine`'s own public rules — a dry [`prepare`] against an empty
/// scope stack and a keychain that holds nothing (the `request_editor::headers`
/// precedent, the narrowest public API that fills `auto_headers` without a send). No
/// send happens and no secret is read.
fn prepared_request() -> Prepared {
    let request = HttpRequest {
        id: ItemId::new(),
        name: String::from("request as sent test"),
        method: Method::Post,
        url: String::from("https://example.test/items"),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![header_row("X-Trace", "trace-1")],
        body: Body::Raw {
            language: RawLanguage::Json,
            text: String::from(r#"{"name":"pustman"}"#),
            extra: BTreeMap::new(),
        },
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    };
    let scopes = ScopeStack::new();
    let store = MemorySecretStore::new();
    let mut dynamic = DynamicValues::new();
    prepare(&request, &[], &scopes, &store, &|_, _| None, &mut dynamic)
        .expect("the fixture request must prepare")
}

/// One enabled header row of an [`HttpRequest`].
fn header_row(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: Some(String::from(value)),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// The `Cookie` header the workspace jar applies to a send to `url` — the value the
/// caller of `TimelineTab::set_request_as_sent` computes — through the jar's own
/// matching, never copied here.
fn jar_cookie(url: &str) -> String {
    let url = Url::parse(url).expect("the fixture URL must parse");
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url,
        &[String::from("session=cookie-REDACTED-example; Path=/")],
    );
    jar.cookie_header_for(&url)
        .expect("the stored cookie must match the URL it was stored for")
}

/// The first row named `label`, as the assertions read the record.
fn row<'a>(rows: &'a [RequestAsSentRow], label: &str) -> Option<&'a RequestAsSentRow> {
    rows.iter().find(|row| row.label.as_str() == label)
}
