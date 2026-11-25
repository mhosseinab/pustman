//! An OAuth 2.0 auth the model cannot map is kept raw, but never with its tokens: they
//! live only in the keychain (REQ-HTTP-07), so neither the saved file nor an export may
//! carry one.

use serde_json::{Value, json};

use crate::{Auth, ImportLimits, export_collection, import_collection};

const TOKENS: [&str; 3] = [
    "token-REDACTED-access",
    "token-REDACTED-refresh",
    "token-REDACTED-id",
];

fn import(auth: Value) -> (crate::Collection, Vec<String>) {
    let document = json!({
        "info": {
            "name": "Tokens",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": [],
        "auth": auth
    });
    let bytes = serde_json::to_vec(&document).expect("the fixture serializes");
    let (collection, report) =
        import_collection(&bytes, &ImportLimits::default()).expect("the fixture imports");
    let notes = report
        .problems
        .into_iter()
        .map(|problem| problem.detail)
        .collect();
    (collection, notes)
}

fn assert_no_token(text: &str) {
    for token in TOKENS {
        assert!(!text.contains(token), "{token} survived: {text}");
    }
}

#[test]
fn unmappable_oauth2_import_keeps_no_token() {
    let shapes = [
        // An unknown grant, array-shaped group.
        json!({
            "type": "oauth2",
            "oauth2": [
                { "key": "grant_type", "value": "urn:ietf:params:oauth:grant-type:device_code" },
                { "key": "accessToken", "value": TOKENS[0], "type": "string" },
                { "key": "refreshToken", "value": TOKENS[1], "type": "string" },
                { "key": "idToken", "value": TOKENS[2], "type": "string" },
                { "key": "tokenName", "value": "dev", "type": "string" }
            ]
        }),
        // An unknown client authentication, object-shaped group, and a token beside it.
        json!({
            "type": "oauth2",
            "accessToken": TOKENS[0],
            "oauth2": {
                "grant_type": "client_credentials",
                "client_authentication": "private_key_jwt",
                "refreshToken": TOKENS[1],
                "idToken": TOKENS[2]
            }
        }),
    ];
    for shape in shapes {
        let (collection, notes) = import(shape);
        assert!(
            matches!(collection.auth, Auth::Unsupported(_)),
            "kept raw: {:?}",
            collection.auth
        );
        let saved = serde_json::to_string(&collection).expect("the collection serializes");
        assert_no_token(&saved);
        assert!(
            saved.contains("oauth2"),
            "the rest is still preserved: {saved}"
        );
        assert!(
            notes.iter().any(|note| note.contains("token not imported")),
            "{notes:?}"
        );
        let (exported, _) = export_collection(&collection);
        assert_no_token(&String::from_utf8_lossy(&exported));
    }

    // A workspace saved before the kinds were modelled holds the raw auth; reading it
    // drops the token the same way.
    let legacy: Auth = serde_json::from_value(json!({
        "type": "oauth2",
        "oauth2": [
            { "key": "grant_type", "value": "device_code" },
            { "key": "accessToken", "value": TOKENS[0] }
        ]
    }))
    .expect("a legacy raw auth reads");
    assert!(matches!(legacy, Auth::Unsupported(_)), "{legacy:?}");
    assert_no_token(&serde_json::to_string(&legacy).expect("the auth serializes"));
}
