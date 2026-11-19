//! [`apply_auth`] turns the effective auth into wire fields (PRD REQ-MODEL-01,
//! REQ-HTTP-06): own auth, inherited auth, and `{{variables}}` in every field.

use std::collections::BTreeMap;

use api_model::{ApiKeyLocation, Auth};

use crate::{AppliedAuth, Header, RequestBody, ResolvedRequest, SendSettings, apply_auth};

fn resolved_request(url: &str) -> ResolvedRequest {
    ResolvedRequest::new(
        api_model::Method::Get,
        url,
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .unwrap()
}

#[test]
fn basic_auth_header_applied() {
    let auth = Auth::Basic {
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
        extra: BTreeMap::new(),
    };
    let mut resolved = resolved_request("https://example.test/profile");
    // A hand-set field of the same name must not survive next to the helper's: two
    // `Authorization` values are not valid HTTP.
    resolved
        .headers
        .push(Header::new("authorization", "Basic stale"));

    apply_auth(&mut resolved, &auth, &[], |value| value.to_string());

    assert_eq!(resolved.headers.len(), 1, "one field per name");
    assert_eq!(resolved.headers[0].name, "Authorization");
    // base64("user:pass") — the RFC 4648 known vector.
    assert_eq!(resolved.headers[0].value, "Basic dXNlcjpwYXNz");
}

#[test]
fn bearer_inherited_from_collection() {
    let collection_auth = Auth::Bearer {
        token: Some("{{token}}".to_string()),
        extra: BTreeMap::new(),
    };
    let folder_auth = Auth::Inherit;
    let mut resolved = resolved_request("https://example.test/orders");

    apply_auth(
        &mut resolved,
        &Auth::Inherit,
        &[&folder_auth, &collection_auth],
        |value| value.replace("{{token}}", "token-REDACTED-example"),
    );

    assert_eq!(resolved.headers.len(), 1);
    assert_eq!(resolved.headers[0].name, "Authorization");
    assert_eq!(resolved.headers[0].value, "Bearer token-REDACTED-example");
}

#[test]
fn api_key_in_query() {
    // The step pins exactly three test names (its Verify runs three), so the header
    // placement shares this table with the query one: one helper, every placement.
    for (location, expect_query) in [
        (
            Some(ApiKeyLocation::Query),
            Some("page=2&api_key=token-REDACTED-example"),
        ),
        (Some(ApiKeyLocation::Header), Some("page=2")),
        (None, Some("page=2")),
    ] {
        let auth = Auth::ApiKey {
            key: Some("api_key".to_string()),
            value: Some("{{secret}}".to_string()),
            location,
            extra: BTreeMap::new(),
        };
        let mut resolved = resolved_request("https://example.test/search?page=2");

        apply_auth(&mut resolved, &auth, &[], |value| {
            value.replace("{{secret}}", "token-REDACTED-example")
        });

        assert_eq!(resolved.url.query(), expect_query, "placement {location:?}");
        if location == Some(ApiKeyLocation::Query) {
            assert!(resolved.headers.is_empty(), "a query key adds no header");
        } else {
            assert_eq!(resolved.headers.len(), 1, "placement {location:?}");
            assert_eq!(resolved.headers[0].name, "api_key");
            assert_eq!(resolved.headers[0].value, "token-REDACTED-example");
        }
    }
}

#[test]
fn test_unsupported_auth_kind_is_reported_and_adds_no_credentials() {
    // The raw object carries a credential; only the kind name may leave this call.
    let unsupported = Auth::Unsupported(serde_json::json!({
        "type": "digest",
        "digest": [{ "key": "password", "value": "s3cret" }],
    }));
    let collection_auth = Auth::Bearer {
        token: Some("token-REDACTED-example".to_string()),
        extra: BTreeMap::new(),
    };
    let mut resolved = resolved_request("https://example.test/secure");

    let outcome = apply_auth(&mut resolved, &unsupported, &[&collection_auth], |value| {
        value.to_string()
    });

    // The request's unsupported kind stops the walk, so the collection's Bearer is
    // never applied either — and the send must be reported, not silent.
    assert_eq!(
        outcome,
        AppliedAuth::Unsupported {
            kind: "digest".to_string()
        }
    );
    assert!(
        resolved.headers.is_empty(),
        "an unsupported kind adds no credentials"
    );
    assert_eq!(resolved.url.query(), None, "the query is unchanged");
    assert!(
        !format!("{outcome:?}").contains("s3cret"),
        "the raw auth never leaves the call"
    );

    // A supported kind reports `Applied` (asserted here because this step may not
    // change the earlier tests).
    let basic = Auth::Basic {
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
        extra: BTreeMap::new(),
    };
    let mut resolved = resolved_request("https://example.test/profile");
    let outcome = apply_auth(&mut resolved, &basic, &[], |value| value.to_string());
    assert_eq!(outcome, AppliedAuth::Applied);
}

#[test]
fn test_api_key_with_empty_name_is_reported_incomplete_and_adds_no_credentials() {
    // The import dropped the "key" field (or `{{name}}` resolved to ""): the user
    // configured API-key auth, but there is no name to place the value under.
    let auth = Auth::ApiKey {
        key: None,
        value: Some("s3cret".to_string()),
        location: Some(ApiKeyLocation::Header),
        extra: BTreeMap::new(),
    };
    let mut resolved = resolved_request("https://example.test/search?page=2");
    resolved
        .headers
        .push(Header::new("Accept", "application/json"));
    let headers_before = resolved.headers.clone();
    let query_before = resolved.url.query().map(str::to_string);

    let outcome = apply_auth(&mut resolved, &auth, &[], |value| value.to_string());

    // The configured auth is incomplete, not the explicit no-auth the user never
    // asked for: S13c's `prepare` reports it instead of sending silently.
    assert_eq!(
        outcome,
        AppliedAuth::Incomplete {
            kind: "api key",
            missing: "key name"
        }
    );
    assert_eq!(
        resolved.headers, headers_before,
        "the headers are unchanged"
    );
    assert_eq!(
        resolved.url.query(),
        query_before.as_deref(),
        "the query is unchanged"
    );
    assert!(
        !format!("{outcome:?}").contains("s3cret"),
        "a field value never leaves the call"
    );
}

#[test]
fn test_bearer_with_absent_or_empty_token_is_reported_incomplete_and_adds_no_credentials() {
    // The import dropped the "token" field, or `{{token}}` resolved to "":
    // `Authorization: Bearer ` carries no credential.
    for token in [None, Some("{{token}}".to_string())] {
        let token_case = format!("{token:?}");
        let auth = Auth::Bearer {
            token,
            extra: BTreeMap::new(),
        };
        let mut resolved = resolved_request("https://example.test/secure");
        resolved
            .headers
            .push(Header::new("Accept", "application/json"));
        let headers_before = resolved.headers.clone();
        let query_before = resolved.url.query().map(str::to_string);

        let outcome = apply_auth(&mut resolved, &auth, &[], |value| {
            value.replace("{{token}}", "")
        });

        // The configured auth is incomplete, not the explicit no-auth the user never
        // asked for: S13c's `prepare` reports it instead of sending silently.
        assert_eq!(
            outcome,
            AppliedAuth::Incomplete {
                kind: "bearer",
                missing: "token"
            },
            "token {token_case}"
        );
        assert_eq!(
            resolved.headers, headers_before,
            "the headers are unchanged"
        );
        assert_eq!(
            resolved.url.query(),
            query_before.as_deref(),
            "the query is unchanged"
        );
    }
}

#[test]
fn test_api_key_with_empty_value_is_reported_incomplete_and_adds_no_credentials() {
    // The name to place it under is there, but the "value" field is absent (or
    // `{{secret}}` resolved to ""): an empty header or `X-Key=` is no credential.
    for (value, location) in [
        (None, Some(ApiKeyLocation::Header)),
        (Some("{{secret}}".to_string()), Some(ApiKeyLocation::Query)),
    ] {
        let auth = Auth::ApiKey {
            key: Some("api_key".to_string()),
            value,
            location,
            extra: BTreeMap::new(),
        };
        let mut resolved = resolved_request("https://example.test/search?page=2");
        resolved
            .headers
            .push(Header::new("Accept", "application/json"));
        let headers_before = resolved.headers.clone();
        let query_before = resolved.url.query().map(str::to_string);

        let outcome = apply_auth(&mut resolved, &auth, &[], |value| {
            value.replace("{{secret}}", "")
        });

        assert_eq!(
            outcome,
            AppliedAuth::Incomplete {
                kind: "api key",
                missing: "key value"
            },
            "placement {location:?}"
        );
        assert_eq!(
            resolved.headers, headers_before,
            "the headers are unchanged"
        );
        assert_eq!(
            resolved.url.query(),
            query_before.as_deref(),
            "the query is unchanged"
        );
    }
}

#[test]
fn test_basic_with_both_fields_empty_is_reported_incomplete_and_adds_no_credentials() {
    // Both fields resolved to empty: `Authorization: Basic Og==` (base64 of ":")
    // carries no credential.
    let auth = Auth::Basic {
        username: None,
        password: Some("{{password}}".to_string()),
        extra: BTreeMap::new(),
    };
    let mut resolved = resolved_request("https://example.test/profile");
    resolved
        .headers
        .push(Header::new("Accept", "application/json"));
    let headers_before = resolved.headers.clone();
    let query_before = resolved.url.query().map(str::to_string);

    let outcome = apply_auth(&mut resolved, &auth, &[], |value| {
        value.replace("{{password}}", "")
    });

    assert_eq!(
        outcome,
        AppliedAuth::Incomplete {
            kind: "basic",
            missing: "username"
        }
    );
    assert_eq!(
        resolved.headers, headers_before,
        "the headers are unchanged"
    );
    assert_eq!(
        resolved.url.query(),
        query_before.as_deref(),
        "the query is unchanged"
    );
}

#[test]
fn test_basic_with_an_empty_password_stays_applied() {
    // RFC 7617 allows an empty password — half a credential is still the credential
    // the user configured. The symmetric empty-username row must not regress either.
    for (username, password, expected) in [
        (
            Some("user".to_string()),
            Some(String::new()),
            "Basic dXNlcjo=",
        ),
        (
            Some(String::new()),
            Some("pass".to_string()),
            "Basic OnBhc3M=",
        ),
    ] {
        let auth = Auth::Basic {
            username,
            password,
            extra: BTreeMap::new(),
        };
        let mut resolved = resolved_request("https://example.test/profile");

        let outcome = apply_auth(&mut resolved, &auth, &[], |value| value.to_string());

        assert_eq!(outcome, AppliedAuth::Applied, "expected {expected}");
        assert_eq!(resolved.headers.len(), 1, "one field per name");
        assert_eq!(resolved.headers[0].name, "Authorization");
        assert_eq!(resolved.headers[0].value, expected);
    }
}
