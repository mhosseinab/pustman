//! A history row never keeps a literal credential of the Digest, OAuth 2.0, AWS SigV4 or
//! JWT Bearer kinds — including the ones Postman keeps in fields the model does not know
//! (REQ-HIST-03). Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::collections::BTreeMap;

use api_model::{
    ApiKeyLocation, Auth, AwsSigV4Auth, Body, DigestAuth, HttpRequest, ItemId, JwtAlgorithm,
    JwtBearerAuth, Method, OAuth2Auth, OAuth2Grant, SecretValues,
};
use serde_json::json;

use super::redacted_parts;

const PEM: &str = "-----BEGIN PRIVATE KEY-----token-REDACTED-unused-pem-----END PRIVATE KEY-----";

fn request_with(auth: Auth) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "extended auth".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn stored(auth: Auth) -> String {
    let (_, document) =
        redacted_parts(&request_with(auth), &SecretValues::new()).expect("the row builds");
    document.to_string()
}

#[test]
fn history_masks_literal_credentials_in_unmodelled_auth_fields() {
    let jwt = stored(Auth::JwtBearer(JwtBearerAuth {
        algorithm: JwtAlgorithm::Hs256,
        secret_or_key: "{{jwt_secret}}".to_owned(),
        payload: "{}".to_owned(),
        header_prefix: "Bearer".to_owned(),
        add_to: ApiKeyLocation::Header,
        extra: BTreeMap::from([("privateKey".to_owned(), json!(PEM))]),
    }));
    assert!(!jwt.contains("token-REDACTED-unused-pem"), "{jwt}");
    assert!(
        jwt.contains("{{jwt_secret}}"),
        "a reference stays restorable: {jwt}"
    );

    let oauth2 = stored(Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "token-REDACTED-client-secret".to_owned(),
        extra: BTreeMap::from([
            (
                "password".to_owned(),
                json!("token-REDACTED-leftover-password"),
            ),
            (
                "tokenRequestParams".to_owned(),
                json!([{ "key": "client_secret", "value": "token-REDACTED-param-secret" }]),
            ),
        ]),
        ..OAuth2Auth::default()
    }));
    for secret in [
        "token-REDACTED-client-secret",
        "token-REDACTED-leftover-password",
        "token-REDACTED-param-secret",
    ] {
        assert!(
            !oauth2.contains(secret),
            "{secret} reached history: {oauth2}"
        );
    }
    assert!(
        oauth2.contains("client_credentials"),
        "the rest is kept: {oauth2}"
    );
}

#[test]
fn history_masks_the_identity_beside_the_secret() {
    // The Basic rule: history masks the identity too; only import and export ignore it.
    let digest = stored(Auth::Digest(DigestAuth {
        username: "digest-user-REDACTED-example".to_owned(),
        password: "{{digest_password}}".to_owned(),
        ..DigestAuth::default()
    }));
    assert!(!digest.contains("digest-user-REDACTED-example"), "{digest}");

    let aws = stored(Auth::AwsSigV4(AwsSigV4Auth {
        access_key: "AKIDREDACTEDEXAMPLE".to_owned(),
        secret_key: "{{aws_secret}}".to_owned(),
        region: "eu-west-1".to_owned(),
        ..AwsSigV4Auth::default()
    }));
    assert!(!aws.contains("AKIDREDACTEDEXAMPLE"), "{aws}");
    assert!(aws.contains("eu-west-1"), "the rest is kept: {aws}");

    let oauth2 = stored(Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::Password {
            username: "grant-user-REDACTED-example".to_owned(),
            password: "{{grant_password}}".to_owned(),
        },
        ..OAuth2Auth::default()
    }));
    assert!(!oauth2.contains("grant-user-REDACTED-example"), "{oauth2}");
}
