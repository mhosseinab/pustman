//! Tests for Postman v2.1 GraphQL bodies mapping to and from `Request::GraphQl` (REQ-IO-01/06).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    Auth, Child, Collection, GraphQlRequest, ItemId, KeyValue, Request,
    postman::{ImportLimits, export_collection, import_collection},
};

fn collection_with(items: Value) -> Vec<u8> {
    let document = json!({
        "info": {
            "name": "GraphQL API",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": items
    });
    serde_json::to_vec(&document).expect("the fixture serializes")
}

fn graphql_item(variables: &str) -> Value {
    json!({
        "name": "List users",
        "event": [{
            "listen": "test",
            "script": { "exec": ["pm.test('ok', () => {})"], "type": "text/javascript" }
        }],
        "protocolProfileBehavior": { "disableBodyPruning": true },
        "request": {
            "method": "POST",
            "description": "Lists every user",
            "header": [
                { "key": "X-Trace", "value": "on" },
                { "key": "X-Off", "value": "1", "disabled": true }
            ],
            "auth": {
                "type": "bearer",
                "bearer": [{ "key": "token", "value": "{{token}}", "type": "string" }]
            },
            "body": {
                "mode": "graphql",
                "graphql": {
                    "query": "query Users($first: Int) { users(first: $first) { id } }",
                    "variables": variables
                }
            },
            "url": "{{baseUrl}}/graphql"
        },
        "response": []
    })
}

fn only_graphql(collection: &Collection) -> &GraphQlRequest {
    match collection.children.as_slice() {
        [Child::Request(Request::GraphQl(graphql))] => graphql,
        other => panic!("expected one GraphQL request, got {other:?}"),
    }
}

fn exported_items(collection: &Collection) -> Vec<Value> {
    let (bytes, report) = export_collection(collection);
    assert!(
        report.warnings.is_empty(),
        "a GraphQL request exports without a warning: {:?}",
        report.warnings
    );
    let document: Value = serde_json::from_slice(&bytes).expect("the export is JSON");
    document
        .get("item")
        .and_then(Value::as_array)
        .cloned()
        .expect("the export has an item array")
}

#[test]
fn graphql_body_imports_as_graphql_request() {
    let bytes = collection_with(json!([graphql_item("{\"first\": 10}")]));
    let (collection, report) =
        import_collection(&bytes, &ImportLimits::default()).expect("the collection imports");

    let graphql = only_graphql(&collection);
    assert_eq!(graphql.name, "List users");
    assert_eq!(graphql.url, "{{baseUrl}}/graphql");
    assert_eq!(
        graphql.query,
        "query Users($first: Int) { users(first: $first) { id } }"
    );
    assert_eq!(graphql.variables, "{\"first\": 10}");
    assert_eq!(graphql.operation_name, None);
    assert_eq!(graphql.headers.len(), 2);
    assert_eq!(graphql.headers[0].key, "X-Trace");
    assert!(
        !graphql.headers[1].enabled,
        "a disabled header stays disabled"
    );
    assert!(
        matches!(&graphql.auth, Auth::Bearer { token, .. } if token.as_deref() == Some("{{token}}")),
        "request auth is imported: {:?}",
        graphql.auth
    );
    assert!(
        graphql.extra.contains_key("event"),
        "the test script is preserved"
    );
    assert!(
        graphql.extra.contains_key("protocolProfileBehavior"),
        "unknown item fields are preserved"
    );
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.detail.contains("not executed")),
        "the preserved script is reported as not executed: {:?}",
        report.problems
    );
    assert_eq!(
        collection.order,
        vec![graphql.id],
        "the request is in the collection order"
    );
}

#[test]
fn graphql_request_exports_to_postman_graphql_body() {
    let graphql = GraphQlRequest {
        id: ItemId::new(),
        name: "Get user".to_owned(),
        url: "https://api.example.test/graphql".to_owned(),
        headers: vec![KeyValue {
            key: "X-Trace".to_owned(),
            value: Some("on".to_owned()),
            description: None,
            enabled: true,
            extra: BTreeMap::new(),
        }],
        auth: Auth::Inherit,
        query: "query User($id: ID!) { user(id: $id) { name } }".to_owned(),
        variables: "{\"id\": \"1\"}".to_owned(),
        operation_name: Some("User".to_owned()),
        settings: None,
        extra: BTreeMap::new(),
    };
    let collection = Collection {
        id: ItemId::new(),
        name: "GraphQL API".to_owned(),
        description: None,
        auth: Auth::Inherit,
        variables: Vec::new(),
        order: vec![graphql.id],
        children: vec![Child::Request(Request::GraphQl(graphql))],
        extra: BTreeMap::new(),
    };

    let items = exported_items(&collection);
    assert_eq!(
        items,
        vec![json!({
            "name": "Get user",
            "request": {
                "method": "POST",
                "url": "https://api.example.test/graphql",
                "header": [{ "key": "X-Trace", "value": "on" }],
                "body": {
                    "mode": "graphql",
                    "graphql": {
                        "query": "query User($id: ID!) { user(id: $id) { name } }",
                        "variables": "{\"id\": \"1\"}",
                        "operationName": "User"
                    }
                }
            }
        })]
    );

    // A Postman item exports back unchanged, unknown fields at every level included.
    let mut original = graphql_item("{\"first\": 10}");
    original["request"]["body"]["options"] = json!({ "graphql": { "x-hint": true } });
    original["request"]["body"]["graphql"]["x-extension"] = json!("kept");
    original["id"] = json!("6b1c2d0e-postman-item-id");
    let (imported, _) = import_collection(
        &collection_with(json!([original.clone()])),
        &ImportLimits::default(),
    )
    .expect("the collection imports");
    assert_eq!(exported_items(&imported), vec![original]);
}

#[test]
fn invalid_variables_text_survives_round_trip() {
    let invalid = "{ \"first\": 10, // half typed";
    let (collection, _) = import_collection(
        &collection_with(json!([graphql_item(invalid)])),
        &ImportLimits::default(),
    )
    .expect("invalid variables text never fails the import");
    assert_eq!(only_graphql(&collection).variables, invalid);

    // Through the workspace request file form and back out to Postman, the text is untouched.
    let request = Request::GraphQl(only_graphql(&collection).clone());
    let saved = serde_json::to_vec(&request).expect("the request serializes");
    let reloaded: Request = serde_json::from_slice(&saved).expect("the request reloads");
    let Request::GraphQl(graphql) = &reloaded else {
        panic!("the saved request reloads as GraphQL: {reloaded:?}");
    };
    assert_eq!(graphql.variables, invalid);

    let mut reloaded_collection = collection.clone();
    reloaded_collection.children = vec![Child::Request(reloaded)];
    let items = exported_items(&reloaded_collection);
    assert_eq!(
        items[0]["request"]["body"]["graphql"]["variables"],
        json!(invalid),
        "the variables stay a string, invalid JSON included"
    );
}
