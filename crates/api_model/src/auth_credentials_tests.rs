//! Tests for `Auth::extended_credentials`: the one list of credential strings history,
//! import and export check for the Digest, OAuth 2.0, AWS SigV4 and JWT Bearer kinds —
//! typed secret fields plus whatever Postman keeps in fields the model does not know.

use std::collections::BTreeMap;

use serde_json::json;

use crate::{
    ApiKeyLocation, Auth, AwsSigV4Auth, DigestAuth, JwtAlgorithm, JwtBearerAuth, OAuth2Auth,
    OAuth2Grant,
};

const PEM: &str = "-----BEGIN PRIVATE KEY-----token-REDACTED-unused-pem-----END PRIVATE KEY-----";

#[test]
fn extended_credentials_include_secrets_postman_keeps_in_unmodelled_fields() {
    // An HS256 token keeps its secret in `secret`; the RS key Postman also exported sits
    // unused in `extra`, and is no less a credential for it.
    let jwt = Auth::JwtBearer(JwtBearerAuth {
        algorithm: JwtAlgorithm::Hs256,
        secret_or_key: "token-REDACTED-hmac".to_owned(),
        payload: "{}".to_owned(),
        header_prefix: "Bearer".to_owned(),
        add_to: ApiKeyLocation::Header,
        extra: BTreeMap::from([
            ("privateKey".to_owned(), json!(PEM)),
            ("queryParamKey".to_owned(), json!("token")),
        ]),
    });
    let values = jwt
        .extended_credentials()
        .expect("JWT Bearer is an extended kind");
    assert!(values.iter().any(|value| value == PEM), "{values:?}");
    assert!(values.iter().any(|value| value == "token-REDACTED-hmac"));
    assert!(
        !values.iter().any(|value| value == "token"),
        "a known harmless field is not a credential: {values:?}"
    );

    // A grant switched away from password keeps the old password in `extra`, and a
    // token request parameter can carry a client secret.
    let oauth2 = Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "{{client_secret}}".to_owned(),
        extra: BTreeMap::from([
            (
                "password".to_owned(),
                json!("token-REDACTED-leftover-password"),
            ),
            (
                "tokenRequestParams".to_owned(),
                json!([{ "key": "client_secret", "value": "token-REDACTED-param-secret" }]),
            ),
            ("tokenName".to_owned(), json!("dev")),
        ]),
        ..OAuth2Auth::default()
    });
    let values = oauth2
        .extended_credentials()
        .expect("OAuth 2.0 is an extended kind");
    for secret in [
        "{{client_secret}}",
        "token-REDACTED-leftover-password",
        "token-REDACTED-param-secret",
    ] {
        assert!(
            values.iter().any(|value| value == secret),
            "{secret}: {values:?}"
        );
    }
    assert!(!values.iter().any(|value| value == "dev"), "{values:?}");

    let password_grant = Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::Password {
            username: "user".to_owned(),
            password: "token-REDACTED-grant-password".to_owned(),
        },
        ..OAuth2Auth::default()
    });
    let values = password_grant.extended_credentials().expect("extended");
    assert!(
        values
            .iter()
            .any(|value| value == "token-REDACTED-grant-password")
    );

    let digest = Auth::Digest(DigestAuth {
        password: "token-REDACTED-digest".to_owned(),
        ..DigestAuth::default()
    });
    assert_eq!(
        digest.extended_credentials(),
        Some(vec!["token-REDACTED-digest".to_owned()])
    );

    let aws = Auth::AwsSigV4(AwsSigV4Auth {
        secret_key: "token-REDACTED-aws".to_owned(),
        session_token: Some("token-REDACTED-session".to_owned()),
        ..AwsSigV4Auth::default()
    });
    let values = aws.extended_credentials().expect("extended");
    assert!(values.iter().any(|value| value == "token-REDACTED-aws"));
    assert!(values.iter().any(|value| value == "token-REDACTED-session"));

    // The kinds with their own checks are not covered here.
    let basic = Auth::Basic {
        username: None,
        password: Some("token-REDACTED-basic".to_owned()),
        extra: BTreeMap::new(),
    };
    assert_eq!(basic.extended_credentials(), None);
}
