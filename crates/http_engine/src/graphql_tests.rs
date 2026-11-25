//! GraphQL over the HTTP engine (REQ-GQL-01): the request becomes `POST application/json
//! {query, variables, operationName}`, `{{variables}}` resolve through the normal
//! `prepare`, and a document's operation names are listed in order.

use std::collections::BTreeMap;

use api_model::{Auth, GraphQlRequest, ItemId, Method, Variable, VariableKind};
use secrets::MemorySecretStore;
use variables::{DynamicValues, ScopeKind, ScopeStack};

use super::{GraphQlError, operation_names, to_http_request};
use crate::{Header, RequestBody, prepare};

fn graphql_request(query: &str, variables: &str, operation_name: Option<&str>) -> GraphQlRequest {
    GraphQlRequest {
        id: ItemId::new(),
        name: "graphql test".to_string(),
        url: "https://{{host}}/graphql".to_string(),
        headers: Vec::new(),
        auth: Auth::Inherit,
        query: query.to_string(),
        variables: variables.to_string(),
        operation_name: operation_name.map(str::to_string),
        settings: None,
        extra: BTreeMap::new(),
    }
}

fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

#[test]
fn posts_query_variables_and_operation_name() {
    let request = graphql_request(
        "query GetUser($id: ID!) { user(id: $id) { name } } # {{tag}}",
        "{\n  \"id\": {{userId}},\n  \"locale\": \"{{locale}}\"\n}",
        Some("GetUser"),
    );
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("host", "api.example.test"),
            variable("userId", "42"),
            variable("locale", "en-GB"),
            variable("tag", "v2"),
        ],
    );

    let http = to_http_request(&request).unwrap();
    let prepared = prepare(
        &http,
        &[],
        &scopes,
        &MemorySecretStore::new(),
        &|_, _| None,
        &mut DynamicValues::new(),
    )
    .unwrap();
    let resolved = prepared.resolved;

    assert_eq!(resolved.method, Method::Post);
    assert_eq!(resolved.url.as_str(), "https://api.example.test/graphql");
    assert!(
        resolved
            .auto_headers
            .contains(&Header::new("Content-Type", "application/json")),
        "the body is sent as application/json: {:?}",
        resolved.auto_headers
    );
    let RequestBody::Bytes(bytes) = &resolved.body else {
        panic!("a GraphQL body is bytes, got {:?}", resolved.body);
    };
    let body: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "query": "query GetUser($id: ID!) { user(id: $id) { name } } # v2",
            "variables": { "id": 42, "locale": "en-GB" },
            "operationName": "GetUser",
        })
    );
}

#[test]
fn invalid_variables_json_reports_line_and_column() {
    let request = graphql_request(
        "query { me { id } }",
        "{\n  \"id\": {{userId}},\n  \"name\" \"x\"\n}",
        None,
    );

    let error = to_http_request(&request).unwrap_err();

    let GraphQlError::InvalidVariables { line, column, .. } = &error;
    assert_eq!((*line, *column), (3, 10));
    let message = error.to_string();
    assert!(
        message.contains("line 3") && message.contains("column 10"),
        "the message names the line and column: {message}"
    );
}

#[test]
fn operation_names_listed_in_document_order() {
    let document = r#"
        # query Commented { x }
        query First($q: String = "mutation InString") { a { query } }
        fragment F on query { b }
        mutation Second { c(arg: """
            subscription InBlockString
        """) }
        { anonymous }
        subscription Third { d }
        query { unnamed }
    "#;

    assert_eq!(
        operation_names(document),
        vec![
            "First".to_string(),
            "Second".to_string(),
            "Third".to_string()
        ]
    );
}
