use crate::{Variable, VariableKind};
use std::collections::BTreeMap;

#[test]
fn secret_variable_never_serializes_value() {
    let variable = Variable {
        key: "api_key".to_string(),
        value: Some("token-REDACTED-example".to_string()),
        kind: VariableKind::Secret,
        enabled: true,
        description: Some("team key".to_string()),
        extra: BTreeMap::new(),
    };

    // The file records only the key, kind and description (REQ-VAR-SHARE-02).
    let output = serde_json::to_value(&variable).unwrap();
    assert!(
        output.get("value").is_none(),
        "a secret value must never reach a file"
    );
    assert_eq!(output["key"], "api_key");
    assert_eq!(output["kind"], "secret");
    assert_eq!(output["description"], "team key");

    // Debug is what a panic or a log line prints, so it is redacted too.
    let debug = format!("{variable:?}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "Debug output must not contain a secret value"
    );

    // A shared variable keeps its value on the wire.
    let shared = Variable {
        kind: VariableKind::Default,
        ..variable
    };
    let output = serde_json::to_value(&shared).unwrap();
    assert_eq!(output["value"], "token-REDACTED-example");
}
