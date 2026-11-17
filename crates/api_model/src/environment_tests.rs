use crate::Environment;

#[test]
fn environment_roundtrip_preserves_unknown_fields() {
    // A secret variable in a file has no value at all (REQ-VAR-SHARE-02); everything an
    // import carried survives the export (REQ-MODEL-02).
    let input = r#"{
        "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "name": "staging",
        "variables": [
            { "key": "base", "value": "https://staging.example.com", "kind": "default", "enabled": true },
            { "key": "api_key", "kind": "secret", "enabled": true, "description": "team key" }
        ],
        "_postman_variable_scope": "environment"
    }"#;

    let environment: Environment = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(environment.variables.len(), 2);
    assert_eq!(environment.variables[1].value, None);

    let output = serde_json::to_value(&environment).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "an environment must round-trip verbatim");
}
