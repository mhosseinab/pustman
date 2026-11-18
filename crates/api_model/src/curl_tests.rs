//! Tests for cURL import and export (PRD REQ-HTTP-13).

use crate::curl::{parse_curl, to_curl};
use crate::{Auth, Body, Method, RawLanguage, RequestSettings};

#[test]
fn parses_method_headers_and_json_body() {
    let request = parse_curl(
        "curl -X POST 'https://api.example.com/users' -H 'Content-Type: application/json' -H 'Accept: application/json' --data-raw '{\"name\":\"bob\"}'",
    )
    .expect("a pasted POST command parses");

    assert_eq!(request.method, Method::Post);
    assert_eq!(request.url, "https://api.example.com/users");
    assert_eq!(
        request
            .headers
            .iter()
            .map(|header| header.key.as_str())
            .collect::<Vec<_>>(),
        ["Content-Type", "Accept"]
    );
    assert!(
        matches!(
            &request.body,
            Body::Raw {
                language: RawLanguage::Json,
                text,
                ..
            } if text == "{\"name\":\"bob\"}"
        ),
        "the JSON body is imported: {:?}",
        request.body
    );
}

#[test]
fn parses_basic_auth_flag() {
    let request = parse_curl("curl -u 'alice:password' 'https://example.com/me'")
        .expect("the command parses");
    assert!(
        matches!(
            &request.auth,
            Auth::Basic {
                username,
                password,
                ..
            } if username.as_deref() == Some("alice") && password.as_deref() == Some("password")
        ),
        "the credentials become Basic auth: {:?}",
        request.auth
    );
}

#[test]
fn parses_form_fields() {
    let request = parse_curl(
        "curl -X POST 'https://example.com/upload' -F 'name=alice' -F 'avatar=@avatar.png;type=image/png'",
    )
    .expect("a multipart command parses");

    let Body::FormData { fields, .. } = &request.body else {
        panic!("-F builds a form-data body: {:?}", request.body);
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].key, "name");
    assert_eq!(fields[0].value.as_deref(), Some("alice"));
    assert!(fields[0].file.is_none());
    assert_eq!(fields[1].key, "avatar");
    assert_eq!(fields[1].file.as_deref(), Some("avatar.png"));
    assert_eq!(fields[1].content_type.as_deref(), Some("image/png"));
}

#[test]
fn insecure_flag_disables_tls_verify() {
    let request = parse_curl("curl -k 'https://example.com/me'").expect("a command with -k parses");
    assert_eq!(
        request.settings,
        Some(RequestSettings {
            verify_tls: Some(false),
            ..RequestSettings::default()
        })
    );
}

#[test]
fn handles_line_continuations() {
    let request = parse_curl(
        r#"curl -X POST 'https://example.com/users' \
  -H 'Content-Type: application/json' \
  --data-raw '{"name":"bob"}'"#,
    )
    .expect("a backslash-continued command parses");
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.headers.len(), 1);
    assert!(
        matches!(&request.body, Body::Raw { text, .. } if text == "{\"name\":\"bob\"}"),
        "the body is read across the continuation: {:?}",
        request.body
    );

    let windows = parse_curl(
        r#"curl 'https://example.com/users' ^
  -H 'Accept: application/json'"#,
    )
    .expect("a caret-continued command parses");
    assert_eq!(windows.url, "https://example.com/users");
    assert_eq!(windows.headers.len(), 1);
}

#[test]
fn to_curl_roundtrips_through_parse() {
    let request = parse_curl(
        "curl -X POST 'https://api.example.com/users?verbose=true' -H 'Content-Type: application/json' -u 'alice:password' -k --data-raw '{\"name\":\"bob\"}'",
    )
    .expect("the command parses");

    let exported = to_curl(&request);
    let mut reparsed = parse_curl(&exported).expect("the export parses back");
    reparsed.id = request.id;
    assert_eq!(
        reparsed, request,
        "the export reads back as the same request: {exported}"
    );
}

#[test]
fn to_curl_escapes_single_quotes() {
    let request = parse_curl(r#"curl -X POST 'https://example.com/greet' --data-raw "it's fine""#)
        .expect("the command parses");

    let exported = to_curl(&request);
    assert!(
        exported.contains(r#"'\''"#),
        "a single quote closes, escapes and reopens: {exported}"
    );
    assert!(
        exported.contains("'it'\\''s fine'"),
        "the body is quoted the POSIX way: {exported}"
    );
    let reparsed = parse_curl(&exported).expect("the export parses back");
    assert!(
        matches!(&reparsed.body, Body::Raw { text, .. } if text == "it's fine"),
        "the quote survives the round trip: {:?}",
        reparsed.body
    );
}
