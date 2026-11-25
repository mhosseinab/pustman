//! The export's literal-credential warning covers the Digest, OAuth 2.0, AWS SigV4 and JWT
//! Bearer kinds, including the unmodelled fields the Postman export writes back verbatim.

use std::collections::BTreeMap;

use api_model::{
    ApiKeyLocation, Auth, Collection, ItemId, JwtAlgorithm, JwtBearerAuth, OAuth2Auth, OAuth2Grant,
};
use serde_json::json;

use super::literal_credentials;

fn collection_with(name: &str, auth: Auth) -> Collection {
    Collection {
        id: ItemId::new(),
        name: name.to_owned(),
        description: None,
        auth,
        variables: Vec::new(),
        order: Vec::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    }
}

#[test]
fn export_warns_about_literal_credentials_in_unmodelled_auth_fields() {
    let jwt = Auth::JwtBearer(JwtBearerAuth {
        algorithm: JwtAlgorithm::Hs256,
        secret_or_key: "{{jwt_secret}}".to_owned(),
        payload: "{}".to_owned(),
        header_prefix: "Bearer".to_owned(),
        add_to: ApiKeyLocation::Header,
        extra: BTreeMap::from([(
            "privateKey".to_owned(),
            json!("-----BEGIN PRIVATE KEY-----token-REDACTED-pem-----END PRIVATE KEY-----"),
        )]),
    });
    assert_eq!(
        literal_credentials(&collection_with("JWT API", jwt)),
        vec!["JWT API".to_owned()]
    );

    let oauth2 = Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "{{client_secret}}".to_owned(),
        extra: BTreeMap::from([(
            "password".to_owned(),
            json!("token-REDACTED-leftover-password"),
        )]),
        ..OAuth2Auth::default()
    });
    assert_eq!(
        literal_credentials(&collection_with("OAuth API", oauth2)),
        vec!["OAuth API".to_owned()]
    );

    let clean = Auth::OAuth2(OAuth2Auth {
        grant: OAuth2Grant::ClientCredentials,
        client_secret: "{{client_secret}}".to_owned(),
        extra: BTreeMap::from([("tokenName".to_owned(), json!("dev"))]),
        ..OAuth2Auth::default()
    });
    assert!(literal_credentials(&collection_with("Clean API", clean)).is_empty());
}
