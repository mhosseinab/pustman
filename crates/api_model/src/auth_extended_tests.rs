//! Tests for the Digest, OAuth 2.0, AWS SigV4 and JWT Bearer auth kinds: our JSON, Postman
//! v2.1 both ways, the legacy flag on the implicit grant, and the kinds that stay raw
//! (REQ-HTTP-06/07).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::ExtendedKind;

use crate::{
    ApiKeyLocation, Auth, AwsSigV4Auth, ClientAuthentication, Collection, DigestAlgorithm,
    DigestAuth, EffectiveAuth, ImportLimits, JwtAlgorithm, JwtBearerAuth, OAuth2Auth, OAuth2Grant,
    SecretRef, effective_auth, export_collection, import_collection,
};

fn collection_with_auth(auth: Value) -> Vec<u8> {
    let document = json!({
        "info": {
            "name": "Signed API",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [],
        "auth": auth
    });
    serde_json::to_vec(&document).expect("the fixture serializes")
}

fn import(auth: Value) -> (Collection, Vec<String>) {
    let (collection, report) =
        import_collection(&collection_with_auth(auth), &ImportLimits::default())
            .expect("the fixture imports");
    let notes = report
        .problems
        .into_iter()
        .map(|problem| problem.detail)
        .collect();
    (collection, notes)
}

/// Export the collection to Postman and import that export again.
fn postman_round_trip(collection: &Collection) -> (Value, Auth) {
    let (bytes, report) = export_collection(collection);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let exported: Value = serde_json::from_slice(&bytes).expect("the export is JSON");
    let (again, _) =
        import_collection(&bytes, &ImportLimits::default()).expect("the export imports");
    (exported["auth"].clone(), again.auth)
}

fn extra(key: &str, value: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([(key.to_owned(), value)])
}

fn digest() -> DigestAuth {
    DigestAuth {
        username: "user".to_owned(),
        password: "token-REDACTED-digest".to_owned(),
        realm: Some("api@example.test".to_owned()),
        nonce: Some("dcd98b7102dd2f0e8b11d0f600bfb0c093".to_owned()),
        algorithm: DigestAlgorithm::Sha256Sess,
        qop: Some("auth".to_owned()),
        extra: extra("opaque", json!("5ccc069c403ebaf9f0171e9517f40e41")),
    }
}

fn oauth2() -> OAuth2Auth {
    OAuth2Auth {
        grant: OAuth2Grant::Password {
            username: "user".to_owned(),
            password: "token-REDACTED-grant-password".to_owned(),
        },
        token_url: "https://auth.example.test/token".to_owned(),
        client_id: "pustman".to_owned(),
        client_secret: "{{client_secret}}".to_owned(),
        scope: "read write".to_owned(),
        audience: Some("https://api.example.test".to_owned()),
        client_authentication: ClientAuthentication::Body,
        token_ref: Some(SecretRef::new("oauth2.token.collection-1")),
        extra: extra("tokenName", json!("dev")),
    }
}

fn aws() -> AwsSigV4Auth {
    AwsSigV4Auth {
        access_key: "AKIDEXAMPLE".to_owned(),
        secret_key: "token-REDACTED-aws-secret".to_owned(),
        session_token: Some("token-REDACTED-aws-session".to_owned()),
        region: "us-east-1".to_owned(),
        service: "execute-api".to_owned(),
        extra: extra("addAuthDataToQuery", json!(false)),
    }
}

fn jwt() -> JwtBearerAuth {
    JwtBearerAuth {
        algorithm: JwtAlgorithm::Rs384,
        secret_or_key: "token-REDACTED-jwt-private-key".to_owned(),
        payload: r#"{"sub":"1234567890"}"#.to_owned(),
        header_prefix: "JWT".to_owned(),
        add_to: ApiKeyLocation::Query,
        extra: extra("queryParamKey", json!("access_token")),
    }
}

/// The typed keys `auth` writes: everything but the tag and its `extra` keys, sorted.
fn fields_of(auth: &Auth) -> Vec<&'static str> {
    let value = serde_json::to_value(auth).expect("the auth serializes");
    let (extra, all): (&BTreeMap<String, Value>, &[&'static str]) = match auth {
        Auth::Digest(kind) => (&kind.extra, DigestAuth::FIELDS),
        Auth::OAuth2(kind) => (&kind.extra, OAuth2Auth::FIELDS),
        Auth::AwsSigV4(kind) => (&kind.extra, AwsSigV4Auth::FIELDS),
        Auth::JwtBearer(kind) => (&kind.extra, JwtBearerAuth::FIELDS),
        other => panic!("not an extended kind: {other:?}"),
    };
    let object = value.as_object().expect("an auth is an object");
    let mut keys: Vec<&'static str> = object
        .keys()
        .filter(|key| *key != "type" && !extra.contains_key(*key))
        .map(|key| {
            all.iter()
                .copied()
                .find(|field| field == key)
                .unwrap_or_else(|| panic!("{key} is written but missing from FIELDS"))
        })
        .collect();
    keys.sort_unstable();
    keys
}

const SECRETS: [&str; 6] = [
    "token-REDACTED-digest",
    "token-REDACTED-grant-password",
    "token-REDACTED-aws-secret",
    "token-REDACTED-aws-session",
    "token-REDACTED-jwt-private-key",
    "token-REDACTED-postman-access",
];

#[test]
fn each_new_kind_round_trips_json() {
    let kinds = [
        (Auth::Digest(digest()), "digest"),
        (Auth::OAuth2(oauth2()), "oauth2"),
        (Auth::AwsSigV4(aws()), "aws_sigv4"),
        (Auth::JwtBearer(jwt()), "jwt_bearer"),
    ];
    for (auth, kind) in kinds {
        let text = serde_json::to_string(&auth).expect("the auth serializes");
        let value: Value = serde_json::from_str(&text).expect("the output is JSON");
        assert_eq!(value["type"], kind, "{text}");
        let back: Auth = serde_json::from_str(&text).expect("the auth reads back");
        assert_eq!(back, auth, "{kind} round-trips through our JSON");

        // Debug output is where a secret leaks into logs: none of them may show there.
        let debug = format!("{auth:?}");
        for secret in SECRETS {
            assert!(
                !debug.contains(secret),
                "{kind} Debug shows a secret: {debug}"
            );
        }

        // Digest is answered by the engine (S55a); the others are not signed yet, so
        // the engine sees only their kind name.
        let (effective, _) = effective_auth(&[&auth]);
        if kind == "digest" {
            assert!(
                matches!(effective, EffectiveAuth::Digest { .. }),
                "{effective:?}"
            );
        } else {
            assert_eq!(
                effective,
                EffectiveAuth::Unsupported(json!({ "type": kind }))
            );
        }
    }

    // `FIELDS` names every typed field, so no `extra` key can shadow one.
    let typed = [
        (fields_of(&Auth::Digest(digest())), DigestAuth::FIELDS),
        (fields_of(&Auth::OAuth2(oauth2())), OAuth2Auth::FIELDS),
        (fields_of(&Auth::AwsSigV4(aws())), AwsSigV4Auth::FIELDS),
        (fields_of(&Auth::JwtBearer(jwt())), JwtBearerAuth::FIELDS),
    ];
    for (written, fields) in typed {
        let mut expected: Vec<&str> = fields.to_vec();
        expected.sort_unstable();
        assert_eq!(written, expected);
    }

    // Defaults: PKCE on, header client authentication, a `Bearer` JWT prefix in the header.
    let bare: Auth = serde_json::from_value(json!({
        "type": "oauth2",
        "grant": { "type": "authorization_code", "auth_url": "https://auth.example.test/authorize" }
    }))
    .expect("a sparse oauth2 auth reads");
    let Auth::OAuth2(bare) = bare else {
        panic!("expected OAuth 2.0, got {bare:?}");
    };
    assert_eq!(
        bare.grant,
        OAuth2Grant::AuthorizationCode {
            auth_url: "https://auth.example.test/authorize".to_owned(),
            use_pkce: true
        }
    );
    assert_eq!(bare.client_authentication, ClientAuthentication::Header);
    assert_eq!(bare.token_ref, None);
    let bare: Auth = serde_json::from_value(json!({ "type": "jwt_bearer" })).expect("reads");
    let Auth::JwtBearer(bare) = bare else {
        panic!("expected JWT Bearer, got {bare:?}");
    };
    assert_eq!(bare.header_prefix, "Bearer");
    assert_eq!(bare.add_to, ApiKeyLocation::Header);
    assert_eq!(bare.algorithm, JwtAlgorithm::Hs256);

    // A workspace saved before this step kept an imported Postman digest raw; it now reads
    // as the modelled kind instead of staying read-only forever.
    let legacy: Auth = serde_json::from_value(json!({
        "type": "digest",
        "digest": [
            { "key": "username", "value": "user", "type": "string" },
            { "key": "password", "value": "{{digest_password}}", "type": "string" }
        ]
    }))
    .expect("a legacy raw digest reads");
    let Auth::Digest(legacy) = legacy else {
        panic!("expected Digest, got {legacy:?}");
    };
    assert_eq!(legacy.username, "user");
    assert_eq!(legacy.password, "{{digest_password}}");
    assert_eq!(legacy.algorithm, DigestAlgorithm::Md5);
}

#[test]
fn postman_oauth2_imports_with_grant_and_pkce_flag() {
    let (collection, notes) = import(json!({
        "type": "oauth2",
        "oauth2": [
            { "key": "grant_type", "value": "authorization_code_with_pkce", "type": "string" },
            { "key": "authUrl", "value": "https://auth.example.test/authorize", "type": "string" },
            { "key": "accessTokenUrl", "value": "https://auth.example.test/token", "type": "string" },
            { "key": "clientId", "value": "pustman", "type": "string" },
            { "key": "clientSecret", "value": "{{client_secret}}", "type": "string" },
            { "key": "scope", "value": "openid", "type": "string" },
            { "key": "client_authentication", "value": "body", "type": "string" },
            { "key": "challengeAlgorithm", "value": "S256", "type": "string" },
            { "key": "accessToken", "value": "token-REDACTED-postman-access", "type": "string" },
            { "key": "refreshToken", "value": "token-REDACTED-postman-access", "type": "string" },
            { "key": "tokenName", "value": "dev", "type": "string" }
        ]
    }));
    let Auth::OAuth2(oauth) = &collection.auth else {
        panic!("expected OAuth 2.0, got {:?}", collection.auth);
    };
    assert_eq!(
        oauth.grant,
        OAuth2Grant::AuthorizationCode {
            auth_url: "https://auth.example.test/authorize".to_owned(),
            use_pkce: true
        }
    );
    assert_eq!(oauth.token_url, "https://auth.example.test/token");
    assert_eq!(oauth.client_id, "pustman");
    assert_eq!(oauth.client_secret, "{{client_secret}}");
    assert_eq!(oauth.scope, "openid");
    assert_eq!(oauth.client_authentication, ClientAuthentication::Body);
    assert_eq!(oauth.extra.get("tokenName"), Some(&json!("dev")));
    // Importing is untrusted and offline: no token is fetched and none is referenced.
    assert_eq!(oauth.token_ref, None);

    // Tokens live only in the keychain: the imported ones never enter the model.
    let saved = serde_json::to_string(&collection).expect("the collection serializes");
    assert!(!saved.contains("token-REDACTED-postman-access"), "{saved}");
    assert!(
        notes.iter().any(|note| note.contains("token")),
        "the dropped token is reported: {notes:?}"
    );
    assert!(
        notes
            .iter()
            .all(|note| !note.contains("token-REDACTED-postman-access")),
        "the report names no token value: {notes:?}"
    );

    // Plain authorization_code is an explicit opt-out of PKCE.
    let (collection, _) = import(json!({
        "type": "oauth2",
        "oauth2": [
            { "key": "grant_type", "value": "authorization_code" },
            { "key": "authUrl", "value": "https://auth.example.test/authorize" }
        ]
    }));
    let Auth::OAuth2(plain) = &collection.auth else {
        panic!("expected OAuth 2.0, got {:?}", collection.auth);
    };
    assert_eq!(
        plain.grant,
        OAuth2Grant::AuthorizationCode {
            auth_url: "https://auth.example.test/authorize".to_owned(),
            use_pkce: false
        }
    );
    assert_eq!(plain.client_authentication, ClientAuthentication::Header);

    // Client credentials and password grants map, and every grant survives an export.
    let (collection, _) = import(json!({
        "type": "oauth2",
        "oauth2": [
            { "key": "grant_type", "value": "client_credentials" },
            { "key": "accessTokenUrl", "value": "https://auth.example.test/token" }
        ]
    }));
    let Auth::OAuth2(credentials) = &collection.auth else {
        panic!("expected OAuth 2.0, got {:?}", collection.auth);
    };
    assert_eq!(credentials.grant, OAuth2Grant::ClientCredentials);

    let mut collection = collection;
    for grant in [
        OAuth2Grant::AuthorizationCode {
            auth_url: "https://auth.example.test/authorize".to_owned(),
            use_pkce: true,
        },
        OAuth2Grant::AuthorizationCode {
            auth_url: "https://auth.example.test/authorize".to_owned(),
            use_pkce: false,
        },
        OAuth2Grant::ClientCredentials,
        OAuth2Grant::Password {
            username: "user".to_owned(),
            password: "{{grant_password}}".to_owned(),
        },
        OAuth2Grant::Implicit {
            auth_url: "https://auth.example.test/authorize".to_owned(),
        },
    ] {
        let auth = OAuth2Auth {
            grant,
            token_ref: None,
            ..oauth2()
        };
        collection.auth = Auth::OAuth2(auth.clone());
        let (exported, again) = postman_round_trip(&collection);
        assert_eq!(exported["type"], "oauth2");
        assert_eq!(again, Auth::OAuth2(auth), "exported as {exported}");
    }

    // A hostile file cannot smuggle a keychain reference or a grant in through `extra`:
    // once saved and reopened, the typed fields are still the ones the import set.
    let (hostile, _) = import(json!({
        "type": "oauth2",
        "oauth2": [
            { "key": "grant_type", "value": "client_credentials" },
            { "key": "token_ref", "value": "another-collection.token" },
            { "key": "grant", "value": { "type": "implicit", "auth_url": "https://evil.test" } },
            { "key": "client_secret", "value": "{{client_secret}}" }
        ]
    }));
    let saved = serde_json::to_string(&hostile.auth).expect("the auth serializes");
    let reopened: Auth = serde_json::from_str(&saved).expect("the auth reads back");
    let Auth::OAuth2(reopened) = reopened else {
        panic!("expected OAuth 2.0, got {reopened:?}");
    };
    assert_eq!(reopened.token_ref, None, "{saved}");
    assert_eq!(reopened.grant, OAuth2Grant::ClientCredentials, "{saved}");
    assert_eq!(reopened.client_secret, "", "{saved}");

    // A keychain reference is local to this machine: an export never carries it.
    collection.auth = Auth::OAuth2(oauth2());
    let (exported, again) = postman_round_trip(&collection);
    assert!(!exported.to_string().contains("oauth2.token.collection-1"));
    let Auth::OAuth2(again) = again else {
        panic!("expected OAuth 2.0, got {again:?}");
    };
    assert_eq!(again.token_ref, None);
}

#[test]
fn implicit_grant_marked_legacy() {
    let (collection, notes) = import(json!({
        "type": "oauth2",
        "oauth2": {
            "grant_type": "implicit",
            "authUrl": "https://auth.example.test/authorize",
            "clientId": "pustman"
        }
    }));
    let Auth::OAuth2(oauth) = &collection.auth else {
        panic!("expected OAuth 2.0, got {:?}", collection.auth);
    };
    assert_eq!(
        oauth.grant,
        OAuth2Grant::Implicit {
            auth_url: "https://auth.example.test/authorize".to_owned()
        }
    );
    assert!(oauth.grant.is_legacy());
    assert!(
        notes.iter().any(|note| note.contains("legacy")),
        "the import says the grant is legacy: {notes:?}"
    );

    for current in [
        OAuth2Grant::AuthorizationCode {
            auth_url: String::new(),
            use_pkce: true,
        },
        OAuth2Grant::ClientCredentials,
        OAuth2Grant::Password {
            username: String::new(),
            password: String::new(),
        },
    ] {
        assert!(!current.is_legacy(), "{current:?} is not legacy");
    }
}

#[test]
fn postman_awsv4_and_jwt_round_trip() {
    let (collection, _) = import(json!({
        "type": "awsv4",
        "awsv4": [
            { "key": "accessKey", "value": "AKIDEXAMPLE", "type": "string" },
            { "key": "secretKey", "value": "{{aws_secret}}", "type": "string" },
            { "key": "sessionToken", "value": "{{aws_session}}", "type": "string" },
            { "key": "region", "value": "eu-west-1", "type": "string" },
            { "key": "service", "value": "s3", "type": "string" },
            { "key": "addAuthDataToQuery", "value": false, "type": "boolean" }
        ]
    }));
    let expected = AwsSigV4Auth {
        access_key: "AKIDEXAMPLE".to_owned(),
        secret_key: "{{aws_secret}}".to_owned(),
        session_token: Some("{{aws_session}}".to_owned()),
        region: "eu-west-1".to_owned(),
        service: "s3".to_owned(),
        extra: extra("addAuthDataToQuery", json!(false)),
    };
    assert_eq!(collection.auth, Auth::AwsSigV4(expected));
    let (exported, again) = postman_round_trip(&collection);
    assert_eq!(exported["type"], "awsv4");
    assert_eq!(again, collection.auth);

    let (collection, _) = import(json!({
        "type": "jwt",
        "jwt": [
            { "key": "algorithm", "value": "HS512", "type": "string" },
            { "key": "secret", "value": "{{jwt_secret}}", "type": "string" },
            { "key": "isSecretBase64Encoded", "value": false, "type": "boolean" },
            { "key": "payload", "value": "{\"sub\":\"42\"}", "type": "string" },
            { "key": "addTokenTo", "value": "queryParam", "type": "string" },
            { "key": "queryParamKey", "value": "token", "type": "string" }
        ]
    }));
    let Auth::JwtBearer(token) = &collection.auth else {
        panic!("expected JWT Bearer, got {:?}", collection.auth);
    };
    assert_eq!(token.algorithm, JwtAlgorithm::Hs512);
    assert_eq!(token.secret_or_key, "{{jwt_secret}}");
    assert_eq!(token.payload, r#"{"sub":"42"}"#);
    assert_eq!(token.header_prefix, "Bearer");
    assert_eq!(token.add_to, ApiKeyLocation::Query);
    assert_eq!(token.extra.get("queryParamKey"), Some(&json!("token")));
    let (exported, again) = postman_round_trip(&collection);
    assert_eq!(exported["type"], "jwt");
    assert_eq!(again, collection.auth);

    // An asymmetric algorithm keeps its key where Postman does: `privateKey`.
    let mut collection = collection;
    collection.auth = Auth::JwtBearer(jwt());
    let (exported, again) = postman_round_trip(&collection);
    let private_key = exported["jwt"]
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["key"] == "privateKey"))
        .map(|entry| entry["value"].clone());
    assert_eq!(private_key, Some(json!("token-REDACTED-jwt-private-key")));
    assert_eq!(again, collection.auth);

    // Digest maps both ways under Postman's own `digest` tag.
    collection.auth = Auth::Digest(digest());
    let (exported, again) = postman_round_trip(&collection);
    assert_eq!(exported["type"], "digest");
    assert_eq!(again, collection.auth);
}

#[test]
fn ntlm_stays_unsupported() {
    let ntlm = json!({
        "type": "ntlm",
        "ntlm": [
            { "key": "username", "value": "user", "type": "string" },
            { "key": "password", "value": "{{ntlm_password}}", "type": "string" }
        ]
    });
    let (collection, notes) = import(ntlm.clone());
    assert_eq!(collection.auth, Auth::Unsupported(ntlm.clone()));
    assert!(notes.iter().any(|note| note.contains("ntlm")), "{notes:?}");
    let (exported, again) = postman_round_trip(&collection);
    assert_eq!(exported, ntlm, "the raw auth exports as it came in");
    assert_eq!(again, collection.auth);

    // The other kinds REQ-HTTP-06 leaves out stay raw in our JSON too.
    for raw in [
        ntlm,
        json!({ "type": "oauth1", "oauth1": [] }),
        json!({ "type": "hawk", "authId": "dh37fgj492je" }),
    ] {
        let auth: Auth = serde_json::from_value(raw.clone()).expect("raw auth reads");
        assert_eq!(auth, Auth::Unsupported(raw));
    }

    // A modelled kind with a value the model has no slot for is preserved raw rather than
    // guessed at.
    let unknown_algorithm = json!({
        "type": "digest",
        "digest": [{ "key": "algorithm", "value": "SHA-512-256", "type": "string" }]
    });
    let (collection, _) = import(unknown_algorithm.clone());
    assert_eq!(collection.auth, Auth::Unsupported(unknown_algorithm));
}
