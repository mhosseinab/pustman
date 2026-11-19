//! Resolving a whole [`HttpRequest`] through `{{variables}}` before a send (PRD §7
//! REQ-VAR-01/02/07): the URL, params, headers and body text come back resolved, names
//! nothing resolves come back as a warning, and one send's dynamic values stay stable.

use std::collections::BTreeMap;
use std::time::Duration;

use api_model::HttpVersion;
use api_model::{
    Auth, Body, HttpRequest, ItemId, KeyValue, Method, RawLanguage, RequestSettings, Variable,
    VariableKind,
};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::error::TransportError;
use crate::pipeline::{PipelineError, Warning, resolve_request};
use crate::request::SendSettings;
use crate::request::{Header, RequestBody};

#[test]
fn resolves_url_headers_and_body() {
    let request = request(
        Method::Post,
        "https://{{host}}/users?inline=1",
        vec![entry("page", "{{page}}")],
        vec![entry("Authorization", "Bearer {{token}}")],
        Body::Raw {
            language: RawLanguage::Json,
            text: r#"{"name": "{{user}}"}"#.to_string(),
            extra: BTreeMap::new(),
        },
    );
    let scopes = scopes(vec![
        variable("host", "example.test"),
        variable("page", "2"),
        variable("token", "token-REDACTED-example"),
        variable("user", "alice"),
    ]);
    let mut dynamic = DynamicValues::new();

    let (resolved, warnings) = resolve_request(&request, &scopes, &mut dynamic).unwrap();

    // The params land on the URL query after whatever it already carried.
    assert_eq!(
        resolved.url.as_str(),
        "https://example.test/users?inline=1&page=2"
    );
    assert_eq!(
        resolved.headers,
        vec![Header::new(
            "Authorization",
            "Bearer token-REDACTED-example"
        )]
    );
    assert_eq!(
        resolved.body,
        RequestBody::Bytes(br#"{"name": "alice"}"#.to_vec())
    );
    assert!(warnings.is_empty(), "everything resolved: {warnings:?}");
}

#[test]
fn unresolved_variable_warns_but_sends() {
    let mut request = request(
        Method::Post,
        "https://example.test/{{path}}",
        vec![entry("q", "{{query}}")],
        vec![entry("X-Trace", "{{trace}}")],
        Body::Raw {
            language: RawLanguage::Text,
            text: "id={{identifier}}".to_string(),
            extra: BTreeMap::new(),
        },
    );
    request.settings = Some(RequestSettings {
        verify_tls: Some(false),
        ..Default::default()
    });
    let mut dynamic = DynamicValues::new();

    let (resolved, warnings) = resolve_request(&request, &ScopeStack::new(), &mut dynamic).unwrap();

    // One warning lists every name once, in first-appearance order across URL, params,
    // headers and body (REQ-VAR-07); the TLS choice is flagged beside it (REQ-HTTP-10).
    assert_eq!(
        warnings,
        vec![
            Warning::UnresolvedVariables(vec![
                "path".to_string(),
                "query".to_string(),
                "trace".to_string(),
                "identifier".to_string(),
            ]),
            Warning::TlsVerifyDisabled,
        ]
    );
    // The request still sends with every placeholder left literal.
    assert_eq!(resolved.url.host_str(), Some("example.test"));
    assert_eq!(resolved.headers, vec![Header::new("X-Trace", "{{trace}}")]);
    assert_eq!(
        resolved.body,
        RequestBody::Bytes(b"id={{identifier}}".to_vec())
    );
    assert!(!resolved.settings.verify_tls);
}

#[test]
fn dynamic_guid_same_across_fields_in_one_send() {
    let request = request(
        Method::Post,
        "https://example.test/hook",
        vec![entry("id", "{{$guid}}")],
        vec![entry("X-Request-Id", "{{$guid}}")],
        Body::Raw {
            language: RawLanguage::Text,
            text: "{{$guid}}".to_string(),
            extra: BTreeMap::new(),
        },
    );
    let mut dynamic = DynamicValues::new();

    let (resolved, warnings) = resolve_request(&request, &ScopeStack::new(), &mut dynamic).unwrap();

    // A dynamic name always resolves, so the warning stays empty (REQ-VAR-05).
    assert!(warnings.is_empty(), "a dynamic name resolves: {warnings:?}");
    let from_query = resolved
        .url
        .query_pairs()
        .find(|(key, _)| key == "id")
        .map(|(_, value)| value.into_owned())
        .expect("the id param is in the query");
    let from_header = &resolved
        .headers
        .first()
        .expect("the request has one header")
        .value;
    let from_body = match &resolved.body {
        RequestBody::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
        other => panic!("expected a text body, got {other:?}"),
    };

    // One DynamicValues is one send: every field carries the same generated value.
    assert!(!from_query.is_empty());
    assert_ne!(from_query, "{{$guid}}");
    assert_eq!(from_header, &from_query);
    assert_eq!(from_body, from_query);
}

#[test]
fn urlencoded_value_with_field_separators_stays_one_field() {
    let request = request(
        Method::Post,
        "https://example.test/form",
        Vec::new(),
        Vec::new(),
        Body::UrlEncoded {
            fields: vec![entry("k", "a&b=c")],
            extra: BTreeMap::new(),
        },
    );
    let mut dynamic = DynamicValues::new();

    let (resolved, _) = resolve_request(&request, &ScopeStack::new(), &mut dynamic).unwrap();

    // A value's own `&` and `=` are percent-encoded, so `a&b=c` stays one field instead
    // of being read as two (`k=a` and a smuggled `b=c`) on the wire.
    assert_eq!(resolved.body, RequestBody::Bytes(b"k=a%26b%3Dc".to_vec()));
}

#[test]
fn invalid_url_error_redacts_the_secret_carrying_value() {
    let request = request(
        Method::Get,
        "https://example.test:{{port}}/x",
        Vec::new(),
        Vec::new(),
        Body::None,
    );
    let scopes = scopes(vec![variable("port", "SECRET-marker")]);
    let mut dynamic = DynamicValues::new();

    let error = resolve_request(&request, &scopes, &mut dynamic).unwrap_err();

    // The URL failure travels as the transport error `ResolvedRequest::new` already
    // produced, whose hand-written `Debug` redacts the resolved text — the marker stands
    // in for a secret variable's value substituted into the URL.
    assert!(matches!(
        error,
        PipelineError::Transport(TransportError::InvalidUrl { .. })
    ));
    let debug = format!("{error:?}");
    assert!(
        !debug.contains("SECRET-marker"),
        "Debug leaked the secret-carrying value: {debug}"
    );
}

/// A secret substituted into the URL reaches no log line: `.log_err()` formats
/// `{error:#}` — `Display`, not `Debug` — so `Display` must carry no field values
/// either. Only `user_message()`, which is for the UI and is never logged, keeps the
/// full sentence.
#[test]
fn a_secret_in_the_url_reaches_the_user_but_never_display_or_debug() {
    let request = request(
        Method::Get,
        "https://example.test:{{port}}/x",
        Vec::new(),
        Vec::new(),
        Body::None,
    );
    let scopes = scopes(vec![variable("port", "token-REDACTED-example")]);
    let mut dynamic = DynamicValues::new();

    let error = resolve_request(&request, &scopes, &mut dynamic).unwrap_err();

    let display = format!("{error:#}");
    assert!(
        !display.contains("token-REDACTED-example"),
        "Display leaked the secret-carrying value: {display}"
    );
    assert!(
        !error.to_string().contains("token-REDACTED-example"),
        "Display leaked the secret-carrying value: {error}"
    );
    let PipelineError::Transport(transport) = &error else {
        panic!("expected a transport error, got {error:?}")
    };
    // The UI still gets the full sentence through `user_message()`.
    assert!(
        transport.user_message().contains("token-REDACTED-example"),
        "the UI lost the real URL: {}",
        transport.user_message()
    );
}

/// Every request-setting override reaches the resolved [`SendSettings`], and a field the
/// request does not override keeps its default. `timeout_ms` is milliseconds
/// (REQ-HTTP-09): `1500` must arrive as `Duration::from_millis(1500)` — 1.5 s — and the
/// first case's equality fails if the unit slips to seconds.
#[test]
fn every_send_settings_override_reaches_the_resolved_request() {
    let cases: Vec<(&str, RequestSettings, SendSettings)> = vec![
        (
            "timeout_ms",
            RequestSettings {
                timeout_ms: Some(1500),
                ..Default::default()
            },
            SendSettings {
                timeout: Duration::from_millis(1500),
                ..SendSettings::default()
            },
        ),
        (
            "follow_redirects",
            RequestSettings {
                follow_redirects: Some(false),
                ..Default::default()
            },
            SendSettings {
                follow_redirects: false,
                ..SendSettings::default()
            },
        ),
        (
            "max_redirects",
            RequestSettings {
                max_redirects: Some(3),
                ..Default::default()
            },
            SendSettings {
                max_redirects: 3,
                ..SendSettings::default()
            },
        ),
        (
            "follow_original_method",
            RequestSettings {
                follow_original_method: Some(true),
                ..Default::default()
            },
            SendSettings {
                follow_original_method: true,
                ..SendSettings::default()
            },
        ),
        (
            "verify_tls",
            RequestSettings {
                verify_tls: Some(false),
                ..Default::default()
            },
            SendSettings {
                verify_tls: false,
                ..SendSettings::default()
            },
        ),
        (
            "encode_url",
            RequestSettings {
                encode_url: Some(false),
                ..Default::default()
            },
            SendSettings {
                encode_url: false,
                ..SendSettings::default()
            },
        ),
        (
            "http_version",
            RequestSettings {
                http_version: Some(HttpVersion::Http2),
                ..Default::default()
            },
            SendSettings {
                http_version: HttpVersion::Http2,
                ..SendSettings::default()
            },
        ),
    ];

    for (name, overrides, expected) in cases {
        let mut request = request(
            Method::Get,
            "https://example.test/x",
            Vec::new(),
            Vec::new(),
            Body::None,
        );
        request.settings = Some(overrides);
        let mut dynamic = DynamicValues::new();

        let (resolved, _) = resolve_request(&request, &ScopeStack::new(), &mut dynamic).unwrap();

        assert_eq!(
            resolved.settings, expected,
            "{name} did not reach the resolved request"
        );
    }
}

/// A disabled param or header row is not sent: the resolver walks enabled rows only, so
/// a disabled row reaches neither the query nor the headers even when its value is one
/// that must never go out.
#[test]
fn a_disabled_param_or_header_row_is_not_sent() {
    let request = request(
        Method::Get,
        "https://example.test/x",
        vec![
            entry("page", "2"),
            KeyValue {
                enabled: false,
                ..entry("api_key", "token-REDACTED-example")
            },
        ],
        vec![
            entry("X-Trace", "1"),
            KeyValue {
                enabled: false,
                ..entry("X-Internal", "token-REDACTED-example")
            },
        ],
        Body::None,
    );
    let mut dynamic = DynamicValues::new();

    let (resolved, _) = resolve_request(&request, &ScopeStack::new(), &mut dynamic).unwrap();

    assert_eq!(resolved.url.query(), Some("page=2"));
    assert_eq!(resolved.headers, vec![Header::new("X-Trace", "1")]);
}

/// A `{{var}}` in a header NAME resolves exactly like one in its value (REQ-VAR-01:
/// every text field of the request goes through the resolver).
#[test]
fn a_variable_inside_a_header_name_resolves() {
    let request = request(
        Method::Get,
        "https://example.test/x",
        Vec::new(),
        vec![entry("X-{{prefix}}-Id", "1")],
        Body::None,
    );
    let scopes = scopes(vec![variable("prefix", "Trace")]);
    let mut dynamic = DynamicValues::new();

    let (resolved, _) = resolve_request(&request, &scopes, &mut dynamic).unwrap();

    assert_eq!(resolved.headers, vec![Header::new("X-Trace-Id", "1")]);
}

/// One request with every table the resolver walks.
fn request(
    method: Method,
    url: &str,
    params: Vec<KeyValue>,
    headers: Vec<KeyValue>,
    body: Body,
) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "pipeline test".to_string(),
        method,
        url: url.to_string(),
        params,
        path_variables: Vec::new(),
        headers,
        body,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// One enabled param or header row.
fn entry(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// One enabled shared variable with a shared value.
fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// A scope stack holding `variables` in the environment scope.
fn scopes(variables: Vec<Variable>) -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Environment, variables);
    scopes
}
