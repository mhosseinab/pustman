//! A [`ResolvedRequest`] carries resolved text, secret values included, so its `Debug`
//! output keeps bodies and header values out of logs (AGENTS.md › Security).

use std::path::PathBuf;

use crate::{Header, RequestBody, ResolvedRequest, SendSettings};

#[test]
fn debug_output_hides_bodies_headers_and_query_values() {
    let request = ResolvedRequest::new(
        api_model::Method::Post,
        "https://example.test/users?api_key=secret-value#token",
        vec![Header::new("Authorization", "Bearer secret-token")],
        RequestBody::Bytes(b"password=hunter2".to_vec()),
        SendSettings::default(),
    )
    .unwrap();

    let debug = format!("{request:?}");

    assert!(
        !debug.contains("secret-value"),
        "query value leaked: {debug}"
    );
    assert!(
        !debug.contains("secret-token"),
        "header value leaked: {debug}"
    );
    assert!(!debug.contains("hunter2"), "body leaked: {debug}");
    assert!(debug.contains("Authorization"), "header names stay visible");
    assert!(
        debug.contains("https://example.test/users"),
        "url stays visible"
    );
}

#[test]
fn debug_output_describes_a_file_body_by_path() {
    let request = ResolvedRequest::new(
        api_model::Method::Put,
        "https://example.test/avatar",
        Vec::new(),
        RequestBody::File(PathBuf::from("./fixtures/avatar.png")),
        SendSettings::default(),
    )
    .unwrap();

    let debug = format!("{request:?}");

    assert!(
        debug.contains("avatar.png"),
        "the path stays visible: {debug}"
    );
}

#[test]
fn debug_output_hides_url_userinfo_credentials() {
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "https://user:pass@host/x",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .unwrap();

    let debug = format!("{request:?}");

    assert!(!debug.contains("user"), "userinfo user leaked: {debug}");
    assert!(!debug.contains("pass"), "userinfo password leaked: {debug}");
    assert!(debug.contains("host/x"), "url stays visible: {debug}");
}
