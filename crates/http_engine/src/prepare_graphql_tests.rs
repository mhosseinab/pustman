//! A resolved value inside the query or the operation name is JSON-escaped on the wire:
//! a `"`, `\` or line break can neither break the body nor add keys to it (REQ-GQL-01).
//! Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{Auth, GraphQlRequest, ItemId, Variable, VariableKind, redact_text};
use gpui::{AppContext, TestAppContext};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use variables::{ScopeKind, ScopeStack};

use crate::graphql::GraphQlSendInput;
use crate::{FakeTransport, HttpClient, RequestBody, SendOwner};

const SECRET: &str = "token-REDACTED-example";

fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

fn secret_key(name: &str) -> SecretKey {
    SecretKey::new("workspace-under-test", "environment", name).unwrap()
}

#[gpui::test]
async fn test_query_variable_containing_quote_and_backslash_is_sent_escaped(
    cx: &mut TestAppContext,
) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"data":{}}"#);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let hostile = "a\"b\\c\nd\"}\",\"variables\":{\"id\":\"other\"},\"_\":\"";
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("text", Some(hostile), VariableKind::Default),
            variable("token", None, VariableKind::Secret),
            variable("op", Some("Op\"Name"), VariableKind::Default),
        ],
    );
    let store = MemorySecretStore::new();
    store
        .set(&secret_key("token"), &SecretValue::new(SECRET))
        .unwrap();
    let input = GraphQlSendInput {
        request: GraphQlRequest {
            id: ItemId::new(),
            name: "escape test".to_string(),
            url: "https://example.test/graphql".to_string(),
            headers: Vec::new(),
            auth: Auth::Inherit,
            query: "query { echo(text: \"{{text}}\", key: \"{{token}}\") }".to_string(),
            variables: "{\"n\": 1}".to_string(),
            operation_name: Some("{{op}}".to_string()),
            settings: None,
            extra: BTreeMap::new(),
        },
        ancestors: Vec::new(),
        scopes,
        secrets: Arc::new(store),
        secret_key: Arc::new(|_, name| Some(secret_key(name))),
    };

    let outcome = client
        .update(cx, |client, cx| {
            client.send_graphql(SendOwner::Shared, input, None, cx)
        })
        .unwrap()
        .await
        .unwrap();

    let sent = transport.requests();
    let RequestBody::Bytes(bytes) = &sent[0].body else {
        panic!("a GraphQL body is bytes");
    };
    let body: serde_json::Value = serde_json::from_slice(bytes).expect("the body is JSON");
    let object = body.as_object().expect("the body is an object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["operationName", "query", "variables"]);
    assert_eq!(
        object["query"],
        serde_json::Value::String(format!(
            "query {{ echo(text: \"{hostile}\", key: \"{SECRET}\") }}"
        ))
    );
    assert_eq!(object["operationName"], "Op\"Name");
    assert_ne!(
        redact_text(SECRET, &outcome.secrets_used),
        SECRET,
        "the secret used in the query is in secrets_used"
    );
}
