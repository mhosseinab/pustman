//! A request's disabled auto headers never reach the transport (REQ-HTTP-03): the
//! setting on the request drops them from the send, and the rest of the auto headers go
//! out unchanged.

use std::sync::Arc;

use api_model::{Body, HttpRequest, ItemId, Method, RequestSettings};
use gpui::{AppContext, TestAppContext};
use secrets::{MemorySecretStore, SecretKey};
use variables::{ScopeKind, ScopeStack};

use crate::{FakeTransport, HttpClient, SendInput};

#[gpui::test]
async fn disabled_auto_header_is_not_sent(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let mut request = plain_request();
    request.settings = Some(RequestSettings {
        // Lowercase on purpose: header names compare ASCII-case-insensitively, so this
        // disables the auto `User-Agent`.
        disabled_auto_headers: vec![String::from("user-agent")],
        ..RequestSettings::default()
    });

    let outcome = client
        .update(cx, |client, cx| {
            client.send_request(send_input(request), cx)
        })
        .await
        .unwrap();

    let sent = transport.requests();
    assert_eq!(sent.len(), 1, "the send reached the transport once");
    let auto_names = auto_header_names(&sent[0].auto_headers);
    assert!(
        auto_names
            .iter()
            .all(|name| !name.eq_ignore_ascii_case("user-agent")),
        "the disabled User-Agent must not be sent: {auto_names:?}"
    );
    assert!(
        auto_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case("accept")),
        "the auto headers the request did not disable still go: {auto_names:?}"
    );

    // What the Console and history record as sent is the same list.
    let reported = auto_header_names(&outcome.request.auto_headers);
    assert_eq!(
        reported, auto_names,
        "the reported request must show exactly what was sent"
    );
}

/// The header names of a request's auto headers, in order.
fn auto_header_names(headers: &[crate::Header]) -> Vec<String> {
    headers.iter().map(|header| header.name.clone()).collect()
}

/// One GET to `https://example.test/users` with no params, no body and no own auth.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "disabled auto header test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: api_model::Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: Default::default(),
    }
}

/// Everything [`HttpClient::send_request`] needs for one send: no secrets to read and
/// no variables to resolve.
fn send_input(request: HttpRequest) -> SendInput {
    SendInput {
        request,
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(lookup),
    }
}

/// No name is bound to a keychain entry: this test sends no secret.
fn lookup(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}
