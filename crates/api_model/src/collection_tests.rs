use crate::{Collection, ItemId, Workspace};
use std::str::FromStr;

#[test]
fn collection_roundtrip_keeps_order_and_unknown_fields() {
    // A collection stores its children's order as ids (REQ-WSP-01) and keeps whatever an
    // import carried but Pustman does not model (REQ-MODEL-02).
    let input = r#"{
        "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "name": "Users API",
        "description": "team collection",
        "auth": { "type": "basic", "username": "user", "password": "token-REDACTED-example" },
        "variables": [
            { "key": "base", "value": "https://example.com", "kind": "default", "enabled": true }
        ],
        "order": ["01BX5ZZKBKACTAV9WEVGEMMVRZ"],
        "x-postman-id": "1234-5678"
    }"#;

    let collection: Collection = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(
        collection.order,
        vec![ItemId::from_str("01BX5ZZKBKACTAV9WEVGEMMVRZ").unwrap()]
    );
    assert!(
        collection.children.is_empty(),
        "children live in their own files, so a parsed collection has none"
    );

    let output = serde_json::to_value(&collection).unwrap();
    let original: serde_json::Value = serde_json::from_slice(input.as_bytes()).unwrap();
    assert_eq!(output, original, "a collection must round-trip verbatim");
}

#[test]
fn workspace_and_folder_roundtrip() {
    let workspace_input = r#"{
        "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "name": "my-api-workspace",
        "format_version": "1.0",
        "x-opened-by": "pustman-v1"
    }"#;
    let workspace: Workspace = serde_json::from_slice(workspace_input.as_bytes()).unwrap();
    assert_eq!(workspace.format_version, "1.0");
    let output = serde_json::to_value(&workspace).unwrap();
    let original: serde_json::Value = serde_json::from_slice(workspace_input.as_bytes()).unwrap();
    assert_eq!(output, original);

    let folder_input = r#"{
        "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        "name": "auth",
        "auth": { "type": "basic", "username": "user", "password": "token-REDACTED-example" },
        "order": ["01ARZ3NDEKTSV4RRFFQ69G5FAV"]
    }"#;
    let folder: crate::Folder = serde_json::from_slice(folder_input.as_bytes()).unwrap();
    assert!(folder.children.is_empty());
    let output = serde_json::to_value(&folder).unwrap();
    let original: serde_json::Value = serde_json::from_slice(folder_input.as_bytes()).unwrap();
    assert_eq!(output, original);

    // `inherit` is the default, so an explicit inherit block is written back as "no block":
    // absence and `{"type":"inherit"}` mean the same thing and files stay free of both.
    let folder: crate::Folder = serde_json::from_slice(
        br#"{ "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ", "name": "auth", "auth": { "type": "inherit" } }"#,
    )
    .unwrap();
    assert!(folder.auth.is_inherit());
    assert_eq!(
        serde_json::to_value(&folder).unwrap(),
        serde_json::json!({
            "id": "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "name": "auth"
        })
    );
}
