//! The two REQ-HTTP-09 overrides that had no typed field — "keep `Authorization`
//! across redirects" and "disable cookie jar" — round-trip as typed [`RequestSettings`]
//! fields, and a request that overrides neither keeps the workspace format it always
//! had (like `disabled_auto_headers`, the empty case never serializes).

use crate::RequestSettings;

#[test]
fn keep_authorization_and_disable_cookie_jar_round_trip_as_typed_fields() {
    // A request that pins both overrides stores them as typed fields, not `extra`.
    let settings: RequestSettings =
        serde_json::from_slice(br#"{"keep_authorization": true, "disable_cookie_jar": true}"#)
            .unwrap();
    assert_eq!(
        settings.keep_authorization,
        Some(true),
        "keep_authorization lands in a typed field"
    );
    assert_eq!(
        settings.disable_cookie_jar,
        Some(true),
        "disable_cookie_jar lands in a typed field"
    );
    assert!(
        settings.extra.is_empty(),
        "a typed override never degrades to the `extra` map"
    );

    // They survive a round trip through the workspace JSON unchanged.
    let json = serde_json::to_string(&settings).unwrap();
    let round_tripped: RequestSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(
        round_tripped, settings,
        "the overrides must persist with the request: {json}"
    );
}

#[test]
fn a_request_that_pins_neither_override_keeps_the_format_it_had() {
    // `None` means "inherit the global default" (REQ-HTTP-09): a settings block that
    // overrides nothing serializes back as the empty object.
    let settings: RequestSettings = serde_json::from_slice(br"{}").unwrap();
    assert_eq!(settings.keep_authorization, None);
    assert_eq!(settings.disable_cookie_jar, None);
    assert_eq!(
        serde_json::to_string(&settings).unwrap(),
        "{}",
        "an empty settings block must stay empty"
    );
}
