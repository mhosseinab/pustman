//! The `Debug` redaction rule for a `form-data` body (PRD §7): a multipart field NAME is
//! as secret-bearing as its value — `{{variable}}` resolution builds field names too
//! (REQ-VAR-01) — so neither [`RequestBody`]'s `Debug` nor [`ResolvedRequest`]'s names a
//! field. Both print the field count and nothing else.

use crate::request::{MultipartField, MultipartValue, RequestBody, ResolvedRequest, SendSettings};

/// A multipart body whose field name stands in for a resolved secret.
fn multipart_body_with_canary_field() -> RequestBody {
    RequestBody::Multipart(vec![
        MultipartField {
            name: "token-REDACTED-example-canary".to_string(),
            content_type: None,
            value: MultipartValue::Text("value-never-printed".to_string()),
        },
        MultipartField {
            name: "upload".to_string(),
            content_type: None,
            value: MultipartValue::Text("value-never-printed".to_string()),
        },
    ])
}

/// A field name carries a resolved secret exactly as its value does (REQ-VAR-01, PRD
/// §7), so both multipart `Debug` surfaces — `RequestBody` itself and `ResolvedRequest`,
/// whose body prints through `BodyForDebug` — show the field count and never a name.
#[test]
fn test_multipart_field_names_never_print_in_debug() {
    let body = multipart_body_with_canary_field();
    let body_debug = format!("{body:?}");
    assert!(
        !body_debug.contains("token-REDACTED-example-canary"),
        "a multipart field name reached the Debug output: {body_debug}"
    );
    assert!(
        !body_debug.contains("upload"),
        "a multipart field name reached the Debug output: {body_debug}"
    );
    assert!(
        body_debug.contains("fields: 2"),
        "the field count went missing: {body_debug}"
    );

    let request = ResolvedRequest::new(
        api_model::Method::Post,
        "https://example.test/send",
        Vec::new(),
        multipart_body_with_canary_field(),
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");
    let request_debug = format!("{request:?}");
    assert!(
        !request_debug.contains("token-REDACTED-example-canary"),
        "a multipart field name reached the Debug output: {request_debug}"
    );
    assert!(
        !request_debug.contains("upload"),
        "a multipart field name reached the Debug output: {request_debug}"
    );
    assert!(
        request_debug.contains("fields: 2"),
        "the field count went missing: {request_debug}"
    );
}
