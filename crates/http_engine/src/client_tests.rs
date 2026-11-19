//! Behavior of [`HttpClient`]: what a send returns, what cancelling does, and what
//! replacing an in-flight send does (PRD REQ-HTTP-12).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AppContext, Entity, Subscription, TestAppContext};

use crate::{
    FakeTransport, HttpClient, RequestBody, ResolvedRequest, SendEvent, SendSettings,
    TransportError,
};

/// A GET for `https://example.test/users` with no headers, no body and default settings.
fn request() -> ResolvedRequest {
    ResolvedRequest::new(
        api_model::Method::Get,
        "https://example.test/users",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .unwrap()
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
async fn send_returns_scripted_response(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 200, r#"{"ok":true}"#);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));

    let outcome = client
        .update(cx, |client, cx| client.send(request(), cx))
        .await;

    let response = outcome.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body.as_bytes(), br#"{"ok":true}"#);
    assert_eq!(transport.requests().len(), 1);
    assert!(!client.read_with(cx, |client, _| client.is_sending()));
}

#[gpui::test]
async fn status_500_is_a_result_not_error(cx: &mut TestAppContext) {
    let transport = FakeTransport::with_response(cx.executor(), 500, "boom");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));

    let outcome = client
        .update(cx, |client, cx| client.send(request(), cx))
        .await;

    let response = outcome.unwrap();
    assert_eq!(response.status, 500);
    assert_eq!(response.body.as_bytes(), b"boom");
    assert_eq!(transport.dropped(), 0);
}

#[gpui::test]
fn cancel_within_50ms_leaves_no_task(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (events, _subscription) = collect_events(cx, &client);

    let _waiter = client.update(cx, |client, cx| client.send(request(), cx));
    assert!(client.read_with(cx, |client, _| client.is_sending()));

    let started = Instant::now();
    client.update(cx, |client, cx| client.cancel(cx));
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(50),
        "cancelling took {elapsed:?}, which is not within 50 ms"
    );
    assert!(
        client.read_with(cx, |client, _| client.pending_send.is_none()),
        "cancelling leaves no task behind"
    );
    assert!(!client.read_with(cx, |client, _| client.is_sending()));
    assert_eq!(transport.requests().len(), 1);
    // Dropping the task cancels it at once; the executor drops its future — the aborted
    // request — on the next pass.
    cx.executor().run_until_parked();
    assert_eq!(
        transport.dropped(),
        1,
        "cancelling drops the transport future, which aborts the request"
    );
    assert_eq!(*events.borrow(), vec![SendEvent::Cancelled]);
}

#[gpui::test]
fn cancelled_send_is_not_retried(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));

    let _waiter = client.update(cx, |client, cx| client.send(request(), cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.executor().run_until_parked();

    assert_eq!(
        transport.requests().len(),
        1,
        "a cancelled send is never retried"
    );
    assert_eq!(transport.dropped(), 1);
    assert!(!client.read_with(cx, |client, _| client.is_sending()));
}

#[gpui::test]
async fn second_send_cancels_first(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "first");
    transport.push_response(Duration::ZERO, 201, "second");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (events, _subscription) = collect_events(cx, &client);

    let first = client.update(cx, |client, cx| client.send(request(), cx));
    let second = client.update(cx, |client, cx| client.send(request(), cx));

    assert_eq!(transport.requests().len(), 2);
    // The replaced task is dropped at once; the executor drops its future on the next pass.
    cx.executor().run_until_parked();
    assert_eq!(
        transport.dropped(),
        1,
        "a second send drops the first send's transport future"
    );

    let first_outcome = first.await;
    assert!(matches!(first_outcome, Err(TransportError::Cancelled)));

    let second_outcome = second.await;
    assert_eq!(second_outcome.unwrap().status, 201);
    assert_eq!(*events.borrow(), vec![SendEvent::Cancelled]);
}
