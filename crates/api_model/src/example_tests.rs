use crate::Example;

#[test]
fn example_roundtrip_preserves_unknown_fields() {
    // Examples carry a request snapshot plus a named response (REQ-MODEL-04), and the fields
    // an import adds but Pustman does not model survive the export (REQ-MODEL-02).
    let input = r#"{
        "name": "200 OK",
        "request": {
            "type": "http",
            "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "name": "Get user",
            "method": "GET",
            "url": "https://example.com/users/1"
        },
        "response": {
            "status": 200,
            "headers": [{ "key": "Content-Type", "value": "application/json", "enabled": true }],
            "body": "{\"id\":1}",
            "statusText": "OK"
        },
        "_postman_example": true
    }"#;

    let example: Example = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(example.name, "200 OK");
    assert_eq!(example.response.status, 200);

    let output = serde_json::to_value(&example).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "an example must round-trip verbatim");
}
