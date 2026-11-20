//! `disabled_auto_headers` in the workspace format (REQ-HTTP-03): a request's switched-off
//! auto headers persist with it, and a request that disables nothing keeps the exact
//! format it always had.

use crate::RequestSettings;

#[test]
fn disabled_auto_header_persists_in_request_settings() {
    // A request that disabled two auto headers stores them as they were listed.
    let settings: RequestSettings =
        serde_json::from_slice(br#"{"disabled_auto_headers":["User-Agent","Accept-Encoding"]}"#)
            .unwrap();
    assert_eq!(
        settings.disabled_auto_headers,
        ["User-Agent", "Accept-Encoding"],
        "the disabled names land in the request settings"
    );

    // They survive a round trip through the workspace JSON unchanged.
    let json = serde_json::to_string(&settings).unwrap();
    let round_tripped: RequestSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(
        round_tripped, settings,
        "the setting must persist with the request: {json}"
    );

    // A request that disables nothing keeps the format it always had: the empty list is
    // the default and is omitted, so the deterministic workspace format is unchanged.
    let plain: RequestSettings = serde_json::from_slice(br"{}").unwrap();
    assert!(plain.disabled_auto_headers.is_empty());
    let json = serde_json::to_string(&plain).unwrap();
    assert_eq!(
        json, "{}",
        "an empty list must stay out of the format: {json}"
    );
}
