use crate::{Body, Method, RawLanguage, Request};
use std::str::FromStr;

#[test]
fn request_roundtrip_preserves_unknown_fields() {
    // An imported request carrying the fields Pustman does not model: they must come back
    // out of the export unchanged (REQ-MODEL-02).
    let input = r#"{
        "type": "http",
        "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "name": "Get user",
        "method": "GET",
        "url": "{{base}}/users/1",
        "params": [
            { "key": "verbose", "value": "true", "description": "more fields", "enabled": true }
        ],
        "headers": [
            { "key": "Accept", "value": "application/json", "enabled": true }
        ],
        "body": { "mode": "raw", "language": "json", "text": "{\"name\":\"{{name}}\"}" },
        "auth": { "type": "bearer", "token": "{{token}}" },
        "settings": { "timeout_ms": 250, "verify_tls": true },
        "scripts": { "prerequest": ["console.log('never run')"] },
        "protocolProfileBehavior": { "disableBodyPruning": true }
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::Http(http) = &request else {
        panic!("type http must decode as an HttpRequest");
    };
    assert!(http.extra.contains_key("scripts"));
    assert!(http.extra.contains_key("protocolProfileBehavior"));

    let output = serde_json::to_value(&request).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "unknown fields must round-trip verbatim");
}

#[test]
fn unknown_request_type_roundtrips() {
    // A request kind v1 does not model (plan D7 grows the enum one milestone at a time).
    let input = r#"{
        "type": "mqtt",
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "Chat",
        "url": "mqtt://broker.example.com:1883",
        "topic": "chat/room-1"
    }"#;

    let request: Request = serde_json::from_slice(input.as_bytes()).unwrap();
    let Request::Unsupported(raw) = &request else {
        panic!("an unknown request type must be kept raw");
    };
    assert_eq!(raw["type"], "mqtt");
    assert_eq!(raw["topic"], "chat/room-1");

    let output = serde_json::to_value(&request).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(
        output, original,
        "an unknown request type must round-trip verbatim"
    );
}

#[test]
fn custom_method_token_validated() {
    // The nine standard verbs are recognized as themselves ...
    assert_eq!(Method::from_token("GET").unwrap(), Method::Get);
    assert_eq!(Method::from_token("CONNECT").unwrap(), Method::Connect);
    // ... anything else is a custom method and kept exactly as written (REQ-HTTP-01).
    assert_eq!(
        Method::from_token("PURGE").unwrap(),
        Method::Custom("PURGE".to_string())
    );
    assert_eq!(
        Method::from_token("M-SEARCH").unwrap(),
        Method::Custom("M-SEARCH".to_string())
    );
    // Method tokens are case-sensitive (RFC 7231 §4.1), so this is a custom method too.
    assert_eq!(
        Method::from_token("get").unwrap(),
        Method::Custom("get".to_string())
    );

    // Non-token text is rejected, in parsing and on the wire.
    assert!(Method::from_token("").is_err());
    assert!(Method::from_token("BAD METHOD").is_err());
    assert!(Method::from_token("GET\r\nX-Injected: 1").is_err());
    assert!(Method::from_str("bad\tmethod").is_err());
    assert!(serde_json::from_slice::<Method>(br#""BAD METHOD""#).is_err());
    assert_eq!(Method::from_token("PURGE").unwrap().as_str(), "PURGE");
}

#[test]
fn body_unsupported_mode_roundtrips_verbatim() {
    let input = r#"{"mode":"graphql","query":"query { me { id } }","variables":{"a":1}}"#;

    let body: Body = serde_json::from_slice(input.as_bytes()).unwrap();
    assert!(matches!(body, Body::Unsupported(_)));

    let output = serde_json::to_value(&body).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(
        output, original,
        "an unknown body mode must round-trip verbatim"
    );

    // A body mode we do model is decoded, not kept raw.
    let raw: Body =
        serde_json::from_slice(br#"{"mode":"raw","language":"json","text":"{}"}"#).unwrap();
    assert!(matches!(
        raw,
        Body::Raw {
            language: RawLanguage::Json,
            ..
        }
    ));
}
