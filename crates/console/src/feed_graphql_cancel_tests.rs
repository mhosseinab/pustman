//! Every Console entry a GraphQL send leaves is listed as `graphql` (REQ-GQL-01): its
//! cancellation, recorded through [`watch`], and its warnings.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Auth, GraphQlRequest, ItemId};
use gpui::{AppContext, TestAppContext};
use http_engine::graphql::{GraphQlSendInput, PROTOCOL, to_http_request};
use http_engine::{FakeTransport, HttpClient, SendOwner, Warning};
use secrets::MemorySecretStore;
use variables::ScopeStack;

use crate::{ConsoleStore, record_warnings_as, watch};

fn graphql_request() -> GraphQlRequest {
    GraphQlRequest {
        id: ItemId::new(),
        name: "graphql cancel test".to_string(),
        url: "https://example.test/graphql".to_string(),
        headers: Vec::new(),
        auth: Auth::Inherit,
        query: "query { me { id } }".to_string(),
        variables: String::new(),
        operation_name: None,
        settings: None,
        extra: BTreeMap::new(),
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(settings::SettingsStore::init(cx, None), Ok(()));
        crate::init(cx);
    });
}

fn protocols(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        cx.global::<ConsoleStore>()
            .entries()
            .iter()
            .map(|entry| entry.protocol.clone())
            .collect()
    })
}

#[gpui::test]
fn cancelled_graphql_send_is_listed_as_graphql(cx: &mut TestAppContext) {
    init(cx);
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let _subscription = cx.update(|cx| watch(&client, cx));
    let input = GraphQlSendInput {
        request: graphql_request(),
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_, _| None),
    };

    let _waiter = client
        .update(cx, |client, cx| {
            client.send_graphql(SendOwner::Shared, input, None, cx)
        })
        .unwrap();
    cx.run_until_parked();
    client.update(cx, |client, cx| client.cancel(cx));
    cx.run_until_parked();

    assert_eq!(protocols(cx), vec![PROTOCOL.to_string()]);
}

#[gpui::test]
fn graphql_warnings_are_listed_as_graphql(cx: &mut TestAppContext) {
    init(cx);
    let request = to_http_request(&graphql_request()).unwrap();

    cx.update(|cx| {
        record_warnings_as(
            PROTOCOL,
            Some(&request),
            &[
                Warning::UnresolvedVariables(vec!["host".to_string()]),
                Warning::TlsVerifyDisabled,
            ],
            cx,
        );
    });

    assert_eq!(
        protocols(cx),
        vec![PROTOCOL.to_string(), PROTOCOL.to_string()]
    );
}
