//! What [`prepare`] tells the user about the auth: an auth field's unresolved
//! `{{names}}` join the request's [`Warning::UnresolvedVariables`] (REQ-VAR-07), and an
//! auth that added no credential — incomplete or unsupported — is a warning of its own,
//! never a silent send (REQ-HTTP-06). [`presend_warnings`], the editor's pre-send check,
//! sees the auth names exactly as [`prepare`] does.

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, ItemId, KeyValue, Method};
use secrets::{MemorySecretStore, SecretKey};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::{Prepared, Warning, prepare, presend_warnings};

/// A GET carrying `auth`, with one header that references `{{trace}}`, which nothing
/// defines either.
fn request_with(auth: Auth) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "auth warning test".to_string(),
        method: Method::Get,
        url: "https://example.test/me".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![KeyValue {
            key: "X-Trace".to_string(),
            value: Some("{{trace}}".to_string()),
            description: None,
            enabled: true,
            extra: BTreeMap::new(),
        }],
        body: Body::None,
        auth,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn no_key(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}

fn prepared(request: &HttpRequest) -> Prepared {
    prepare(
        request,
        &[],
        &ScopeStack::new(),
        &MemorySecretStore::new(),
        &no_key,
        &mut DynamicValues::new(),
    )
    .expect("the request prepares")
}

fn bearer(token: &str) -> Auth {
    Auth::Bearer {
        token: Some(token.to_string()),
        extra: BTreeMap::new(),
    }
}

#[test]
fn test_an_unresolved_bearer_token_is_named_in_the_unresolved_warning() {
    let prepared = prepared(&request_with(bearer("{{token}}")));

    assert!(
        prepared
            .warnings
            .contains(&Warning::UnresolvedVariables(vec![
                "trace".to_string(),
                "token".to_string()
            ])),
        "one warning names the request's and the auth's unresolved names: {:?}",
        prepared.warnings
    );
}

#[test]
fn test_the_presend_check_names_the_unresolved_auth_variable() {
    let request = request_with(bearer("{{token}}"));

    let warnings = presend_warnings(&request, &[], &ScopeStack::new(), &mut DynamicValues::new())
        .expect("the request resolves");

    assert_eq!(
        warnings,
        vec![Warning::UnresolvedVariables(vec![
            "trace".to_string(),
            "token".to_string()
        ])]
    );
}

#[test]
fn test_the_presend_check_follows_an_inherited_auth() {
    let mut request = request_with(Auth::Inherit);
    request.headers.clear();
    let parent = bearer("{{collection_token}}");

    let warnings = presend_warnings(
        &request,
        &[&parent],
        &ScopeStack::new(),
        &mut DynamicValues::new(),
    )
    .expect("the request resolves");

    assert_eq!(
        warnings,
        vec![Warning::UnresolvedVariables(vec![
            "collection_token".to_string()
        ])]
    );
}

#[test]
fn test_empty_basic_auth_is_reported_as_an_incomplete_auth_warning() {
    let prepared = prepared(&request_with(Auth::Basic {
        username: Some(String::new()),
        password: None,
        extra: BTreeMap::new(),
    }));

    assert!(
        prepared.warnings.contains(&Warning::AuthIncomplete {
            kind: "basic",
            missing: "username"
        }),
        "the incomplete auth reaches the warnings: {:?}",
        prepared.warnings
    );
}

#[test]
fn test_an_unsupported_auth_is_reported_as_a_warning() {
    let prepared = prepared(&request_with(Auth::Unsupported(serde_json::json!({
        "type": "oauth2",
        "accessToken": "token-REDACTED-example"
    }))));

    assert!(
        prepared.warnings.contains(&Warning::AuthUnsupported {
            kind: "oauth2".to_string()
        }),
        "the unsupported auth reaches the warnings: {:?}",
        prepared.warnings
    );
}

#[test]
fn test_auth_warnings_name_no_credential_value() {
    for warning in [
        Warning::AuthIncomplete {
            kind: "basic",
            missing: "username",
        },
        Warning::AuthUnsupported {
            kind: "oauth2".to_string(),
        },
    ] {
        let message = warning.user_message();
        assert!(
            message.contains("without credentials"),
            "the message says what happened: {message}"
        );
        assert!(!message.contains("token-REDACTED-example"));
    }
}
