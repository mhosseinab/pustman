use crate::{Auth, AuthSource, EffectiveAuth, effective_auth};
use std::collections::BTreeMap;

fn basic_auth() -> Auth {
    Auth::Basic {
        username: Some("user".to_string()),
        password: Some("token-REDACTED-example".to_string()),
        extra: BTreeMap::new(),
    }
}

fn bearer_auth() -> Auth {
    Auth::Bearer {
        token: Some("token-REDACTED-example".to_string()),
        extra: BTreeMap::new(),
    }
}

#[test]
fn auth_inherits_from_folder_then_collection() {
    let request = Auth::Inherit;
    let folder = Auth::Inherit;
    let collection = basic_auth();

    // Nothing at request or folder level: the collection's auth is used (REQ-MODEL-01).
    let (effective, source) = effective_auth(&[&request, &folder, &collection]);
    assert!(matches!(effective, EffectiveAuth::Basic { .. }));
    assert_eq!(source, AuthSource::Collection);

    // A folder setting auth wins over the collection.
    let folder = bearer_auth();
    let (effective, source) = effective_auth(&[&request, &folder, &collection]);
    assert!(matches!(effective, EffectiveAuth::Bearer { .. }));
    assert_eq!(source, AuthSource::Folder);

    // And the request's own auth wins over both.
    let request = Auth::ApiKey {
        key: Some("X-Api-Key".to_string()),
        value: Some("token-REDACTED-example".to_string()),
        location: Some(crate::ApiKeyLocation::Header),
        extra: BTreeMap::new(),
    };
    let (effective, source) = effective_auth(&[&request, &folder, &collection]);
    assert!(matches!(effective, EffectiveAuth::ApiKey { .. }));
    assert_eq!(source, AuthSource::Request);

    // Nothing set anywhere means no credentials.
    let (effective, source) = effective_auth(&[&Auth::Inherit, &Auth::Inherit]);
    assert_eq!(effective, EffectiveAuth::None);
    assert_eq!(source, AuthSource::Default);
}

#[test]
fn auth_none_stops_inheritance() {
    let request = Auth::Inherit;
    let folder = Auth::None;
    let collection = basic_auth();

    // "No auth" on a folder is an explicit override, not a missing setting.
    let (effective, source) = effective_auth(&[&request, &folder, &collection]);
    assert_eq!(effective, EffectiveAuth::None);
    assert_eq!(source, AuthSource::Folder);

    // The same at request level: the collection's basic auth must not leak through.
    let (effective, source) = effective_auth(&[&Auth::None, &collection]);
    assert_eq!(effective, EffectiveAuth::None);
    assert_eq!(source, AuthSource::Request);
}

#[test]
fn auth_unsupported_type_roundtrips_verbatim() {
    // Hawk is one of the auth types REQ-MODEL-02 names: preserved, never dropped.
    let input = r#"{"type":"hawk","id":"dh37fgj492je","key":"token-REDACTED-example","algorithm":"sha256"}"#;

    let auth: Auth = serde_json::from_slice(input.as_bytes()).unwrap();
    assert!(matches!(auth, Auth::Unsupported(_)));

    let output = serde_json::to_value(&auth).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(
        output, original,
        "an unsupported auth type must round-trip verbatim"
    );
}
