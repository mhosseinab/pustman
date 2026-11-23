//! Sends from different owners — two request tabs sharing the app's one [`HttpClient`] —
//! run side by side: a send replaces and cancels only its own owner's previous send
//! (REQ-HTTP-12), and cancelling one owner's send leaves every other owner's running.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, Entity, Subscription, TestAppContext};
use secrets::{MemorySecretStore, SecretKey};
use variables::{ScopeKind, ScopeStack};

use crate::{
    FakeTransport, HttpClient, PipelineError, SendEvent, SendInput, SendOwner, TransportError,
};

/// A plain GET to `url`, no auth.
fn request(url: &str) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "concurrent send test".to_string(),
        method: Method::Get,
        url: url.to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: Default::default(),
    }
}

/// Everything one send needs, with no variables and no secrets.
fn send_input(url: &str) -> SendInput {
    SendInput {
        request: request(url),
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_kind: ScopeKind, _name: &str| -> Option<SecretKey> { None }),
    }
}

/// Two owners, as two request tabs are: each an entity of its own.
fn two_owners(cx: &mut TestAppContext) -> (SendOwner, SendOwner) {
    let first = cx.new(|_| ());
    let second = cx.new(|_| ());
    (
        SendOwner::from(first.entity_id()),
        SendOwner::from(second.entity_id()),
    )
}

/// Collect the [`SendEvent`]s `client` emits, for the life of the subscription.
fn collect_events(
    cx: &mut TestAppContext,
    client: &Entity<HttpClient>,
) -> (Rc<RefCell<Vec<SendEvent>>>, Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(client, move |_, event: &SendEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });
    (events, subscription)
}

#[gpui::test]
fn test_two_owners_sending_at_once_both_get_their_own_response(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(2), 200, "slow");
    transport.push_response(Duration::from_secs(1), 201, "fast");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (events, _subscription) = collect_events(cx, &client);
    let (tab_a, tab_b) = two_owners(cx);

    let slow = client.update(cx, |client, cx| {
        client.send_request_as(tab_a, send_input("https://example.test/slow"), cx)
    });
    let fast = client.update(cx, |client, cx| {
        client.send_request_as(tab_b, send_input("https://example.test/fast"), cx)
    });
    cx.run_until_parked();
    assert!(client.read_with(cx, |client, _| client.is_sending_as(tab_a)));
    assert!(client.read_with(cx, |client, _| client.is_sending_as(tab_b)));

    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();

    let slow = cx
        .foreground_executor()
        .block_test(slow)
        .expect("the first tab's send completes");
    let fast = cx
        .foreground_executor()
        .block_test(fast)
        .expect("the second tab's send completes");
    assert_eq!(slow.response.status, 200);
    assert_eq!(fast.response.status, 201);
    assert_eq!(transport.dropped(), 0, "neither send was cancelled");
    assert!(events.borrow().is_empty(), "no cancellation was reported");
}

#[gpui::test]
fn test_cancelling_one_owner_leaves_the_other_owners_send_running(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(1), 200, "cancelled one");
    transport.push_response(Duration::from_secs(1), 202, "kept one");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (events, _subscription) = collect_events(cx, &client);
    let (tab_a, tab_b) = two_owners(cx);

    let cancelled = client.update(cx, |client, cx| {
        client.send_request_as(tab_a, send_input("https://example.test/a"), cx)
    });
    let kept = client.update(cx, |client, cx| {
        client.send_request_as(tab_b, send_input("https://example.test/b"), cx)
    });
    cx.run_until_parked();
    client.update(cx, |client, cx| client.cancel_as(tab_a, cx));
    cx.run_until_parked();
    assert!(!client.read_with(cx, |client, _| client.is_sending_as(tab_a)));
    assert!(
        client.read_with(cx, |client, _| client.is_sending_as(tab_b)),
        "the other tab's send is still in flight"
    );

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    let cancelled = cx.foreground_executor().block_test(cancelled);
    assert!(matches!(
        cancelled,
        Err(PipelineError::Transport(TransportError::Cancelled))
    ));
    let kept = cx
        .foreground_executor()
        .block_test(kept)
        .expect("the other tab's send completes");
    assert_eq!(kept.response.status, 202);
    assert_eq!(
        transport.dropped(),
        1,
        "exactly the cancelled send was dropped"
    );
    let events = events.borrow();
    assert_eq!(events.len(), 1, "one cancellation was reported");
    assert_eq!(
        events[0].send.as_ref().map(|send| send.url.as_str()),
        Some("https://example.test/a"),
        "the cancellation names the cancelled tab's send"
    );
}

#[gpui::test]
fn test_a_second_send_from_the_same_owner_still_cancels_the_first(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(1), 200, "replaced");
    transport.push_response(Duration::from_secs(1), 204, "");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (tab_a, _tab_b) = two_owners(cx);

    let replaced = client.update(cx, |client, cx| {
        client.send_request_as(tab_a, send_input("https://example.test/first"), cx)
    });
    cx.run_until_parked();
    let latest = client.update(cx, |client, cx| {
        client.send_request_as(tab_a, send_input("https://example.test/second"), cx)
    });
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    assert!(matches!(
        cx.foreground_executor().block_test(replaced),
        Err(PipelineError::Transport(TransportError::Cancelled))
    ));
    let latest = cx
        .foreground_executor()
        .block_test(latest)
        .expect("the latest send completes");
    assert_eq!(latest.response.status, 204);
    assert_eq!(transport.dropped(), 1);
}
