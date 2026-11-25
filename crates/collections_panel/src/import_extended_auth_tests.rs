//! The import's literal-credential report covers the Digest, OAuth 2.0, AWS SigV4 and JWT
//! Bearer kinds, including what Postman keeps in fields the model does not know.

use std::collections::BTreeMap;

use api_model::{ApiKeyLocation, Auth, JwtAlgorithm, JwtBearerAuth, OAuth2Auth, OAuth2Grant};
use serde_json::json;

use super::auth_carries_credential;

fn jwt(extra: BTreeMap<String, serde_json::Value>) -> Auth {
    Auth::JwtBearer(JwtBearerAuth {
        algorithm: JwtAlgorithm::Hs256,
        secret_or_key: "{{jwt_secret}}".to_owned(),
        payload: "{}".to_owned(),
        header_prefix: "Bearer".to_owned(),
        add_to: ApiKeyLocation::Header,
        extra,
    })
}

#[test]
fn import_reports_literal_credentials_in_unmodelled_auth_fields() {
    let unused_key = jwt(BTreeMap::from([(
        "privateKey".to_owned(),
        json!("-----BEGIN PRIVATE KEY-----token-REDACTED-pem-----END PRIVATE KEY-----"),
    )]));
    assert!(auth_carries_credential(&unused_key));

    let leftover_password = Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "{{client_secret}}".to_owned(),
        extra: BTreeMap::from([(
            "password".to_owned(),
            json!("token-REDACTED-leftover-password"),
        )]),
        ..OAuth2Auth::default()
    });
    assert!(auth_carries_credential(&leftover_password));

    // Only references and known harmless fields: nothing to report.
    let clean = jwt(BTreeMap::from([(
        "queryParamKey".to_owned(),
        json!("token"),
    )]));
    assert!(!auth_carries_credential(&clean));
}

/// An OAuth 2.0 auth whose secrets are all references, with Postman's usual literal
/// companions and one token request parameter.
fn oauth2_with_token_param(key: &str, value: &str) -> Auth {
    Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "{{client_secret}}".to_owned(),
        extra: BTreeMap::from([
            ("state".to_owned(), json!("state-1234")),
            (
                "refreshTokenUrl".to_owned(),
                json!("https://auth.example.test/refresh"),
            ),
            (
                "tokenRequestParams".to_owned(),
                json!([{ "key": key, "value": value, "send_as": "request_body" }]),
            ),
        ]),
        ..OAuth2Auth::default()
    })
}

#[test]
fn import_judges_oauth2_request_parameters_by_their_name() {
    assert!(!auth_carries_credential(&oauth2_with_token_param(
        "resource",
        "https://api.example.test"
    )));
    assert!(auth_carries_credential(&oauth2_with_token_param(
        "client_secret",
        "token-REDACTED-param-secret"
    )));
}
