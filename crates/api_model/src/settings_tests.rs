use crate::{HttpVersion, RequestSettings};

#[test]
fn settings_none_means_inherit() {
    // Every field is optional and `None` means "use the global default" (REQ-HTTP-09), so a
    // settings block that overrides nothing serializes back as the empty object.
    let settings: RequestSettings = serde_json::from_slice(br"{}").unwrap();
    assert_eq!(settings.timeout_ms, None);
    assert_eq!(settings.follow_redirects, None);
    assert_eq!(settings.max_redirects, None);
    assert_eq!(settings.follow_original_method, None);
    assert_eq!(settings.verify_tls, None);
    assert_eq!(settings.encode_url, None);
    assert_eq!(settings.remove_referer, None);
    assert_eq!(settings.http_version, None);
    assert_eq!(
        serde_json::to_value(&settings).unwrap(),
        serde_json::json!({}),
        "an empty settings block must stay empty"
    );

    // A block only overrides what it names; the rest still inherits.
    let settings: RequestSettings =
        serde_json::from_slice(br#"{"timeout_ms": 250, "http_version": "1.1"}"#).unwrap();
    assert_eq!(settings.timeout_ms, Some(250));
    assert_eq!(settings.http_version, Some(HttpVersion::Http11));
    assert_eq!(settings.verify_tls, None);
    assert_eq!(settings.max_redirects, None);
}
