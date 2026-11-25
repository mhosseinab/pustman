//! An event-stream response's lifecycle beyond the parser (REQ-SSE-01, REQ-HTTP-12): the
//! client's cancel reaches a stream that outlived its send, another owner's send never
//! stops it, the client never keeps a dropped response's stream alive, the request
//! timeout spares an event stream yet still bounds every other body, and a slow reader
//! never makes the pump buffer more than its byte budget.
//!
//! `TestServer` answers whole bodies only, so the streaming answers come from a loopback
//! socket scripted here: the response head, then writes and pauses as each test says.
//! Every wait is bounded, so a regression fails instead of hanging.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use api_model::{Auth, Body, HttpRequest, ItemId, Method, Payload, SessionEvent};
use futures::channel::oneshot;
use gpui::{AppContext, Entity, TestAppContext};
use http_engine::event_stream::{EventStream, MAX_QUEUED_BYTES};
use http_engine::{
    HttpClient, RequestBody, ReqwestTransport, ResolvedRequest, Response, ResponseBody, SendInput,
    SendOwner, SendSettings, TestServer, Transport, TransportError,
};
use secrets::MemorySecretStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;
use variables::ScopeStack;

/// Long enough for a loopback exchange on a loaded machine; a stream left running fails
/// the test after it instead of hanging it.
const WAIT: Duration = Duration::from_secs(10);

const SSE_HEAD: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
    cache-control: no-cache\r\nconnection: close\r\n\r\n";

/// One thing the scripted server does, in order.
enum Step {
    Write(Vec<u8>),
    Pause(Duration),
    /// Keep the connection open until the client closes it, then report that.
    AwaitClientClose(oneshot::Sender<()>),
}

/// Serve one connection on `127.0.0.1`: read the request head, then run `steps`. The
/// connection closes when the steps are done. Must run inside a tokio runtime.
async fn serve(steps: Vec<Step>) -> Url {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a loopback listener");
    let address = listener.local_addr().expect("the bound address");
    tokio::spawn(async move {
        let (mut socket, _peer) = listener.accept().await.expect("one connection");
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket.read(&mut buffer).await.expect("the request head");
            assert!(read > 0, "the client closed before sending its request");
            request.extend_from_slice(&buffer[..read]);
        }
        for step in steps {
            match step {
                Step::Write(bytes) => {
                    // A client that already went away ends the script.
                    if socket.write_all(&bytes).await.is_err() {
                        return;
                    }
                    if socket.flush().await.is_err() {
                        return;
                    }
                }
                Step::Pause(duration) => tokio::time::sleep(duration).await,
                Step::AwaitClientClose(closed) => {
                    // A read of zero bytes (or a reset) is the client closing.
                    loop {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                    closed.send(()).expect("the test waits for the close");
                }
            }
        }
    });
    Url::parse(&format!("http://{address}/events")).expect("a loopback URL")
}

fn sse_then(events: &[u8], closed: oneshot::Sender<()>) -> Vec<Step> {
    vec![
        Step::Write(SSE_HEAD.to_vec()),
        Step::Write(events.to_vec()),
        Step::AwaitClientClose(closed),
    ]
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a tokio runtime")
}

/// Block until the scripted server reports the client's close, failing after [`WAIT`].
fn wait_for_close(runtime: &tokio::runtime::Runtime, closed: oneshot::Receiver<()>, why: &str) {
    runtime
        // The timer is created inside the runtime, which owns the clock it needs.
        .block_on(async { tokio::time::timeout(WAIT, closed).await })
        .unwrap_or_else(|_elapsed| panic!("{why}: the server never saw the connection close"))
        .expect("the server reports the close");
}

fn event_stream_of(response: &Response) -> EventStream {
    match &response.body {
        ResponseBody::EventStream(stream) => stream.clone(),
        other => panic!("an event-stream response carries an event stream, got {other:?}"),
    }
}

fn request(url: &Url, timeout: Duration) -> ResolvedRequest {
    ResolvedRequest::new(
        Method::Get,
        url.as_str(),
        Vec::new(),
        RequestBody::None,
        SendSettings {
            timeout,
            ..SendSettings::default()
        },
    )
    .expect("a resolvable URL")
}

/// One GET for [`HttpClient::send_request_as`]; nothing references a secret.
fn send_input(url: &Url) -> SendInput {
    SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "event stream lifecycle".to_string(),
            method: Method::Get,
            url: url.to_string(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        },
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_, _| None),
    }
}

/// A client over the real transport on `runtime`, and two tab owners.
fn client_and_owners(
    cx: &mut TestAppContext,
    runtime: &tokio::runtime::Runtime,
) -> (Entity<HttpClient>, SendOwner, SendOwner) {
    let transport = ReqwestTransport::new(runtime.handle().clone());
    let client = cx.new(|_| HttpClient::new(Arc::new(transport)));
    let tab_a = cx.new(|_| ());
    let tab_b = cx.new(|_| ());
    (
        client,
        SendOwner::Entity(tab_a.entity_id()),
        SendOwner::Entity(tab_b.entity_id()),
    )
}

async fn send_as(
    cx: &mut TestAppContext,
    client: &Entity<HttpClient>,
    owner: SendOwner,
    url: &Url,
) -> Response {
    client
        .update(cx, |client, cx| {
            client.send_request_as(owner, send_input(url), cx)
        })
        .await
        .expect("the send succeeds")
        .response
}

#[gpui::test]
async fn cancel_as_stops_an_open_stream_and_the_server_sees_the_close(cx: &mut TestAppContext) {
    // The sends cross real sockets on a tokio runtime: the scheduler may park for them.
    cx.executor().allow_parking();
    let runtime = runtime();
    let (closed, client_closed) = oneshot::channel();
    let url = runtime.block_on(serve(sse_then(b"data: a\n\n", closed)));
    let (client, tab_a, _tab_b) = client_and_owners(cx, &runtime);

    let response = send_as(cx, &client, tab_a, &url).await;
    let stream = event_stream_of(&response);
    client.update(cx, |client, cx| client.cancel_as(tab_a, cx));

    wait_for_close(&runtime, client_closed, "cancel_as(A)");
    assert!(stream.is_stopped(), "the cancel stopped A's stream");
}

#[gpui::test]
async fn another_owners_send_leaves_the_stream_open_and_its_owner_can_still_cancel_it(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let runtime = runtime();
    let (closed, mut client_closed) = oneshot::channel();
    let url = runtime.block_on(serve(sse_then(b"data: a\n\n", closed)));
    let echo = runtime
        .block_on(TestServer::start())
        .expect("the echo server starts");
    let (client, tab_a, tab_b) = client_and_owners(cx, &runtime);

    let response = send_as(cx, &client, tab_a, &url).await;
    let stream = event_stream_of(&response);
    let other = send_as(cx, &client, tab_b, echo.url()).await;
    assert_eq!(other.status, 200, "B's send completes");

    assert!(stream.is_open(), "B's send left A's stream open");
    assert_eq!(
        client_closed
            .try_recv()
            .expect("the server is still waiting"),
        None,
        "A's connection is still open after B's send"
    );
    client.update(cx, |client, cx| client.cancel_as(tab_a, cx));
    wait_for_close(&runtime, client_closed, "cancel_as(A) after B's send");
}

#[gpui::test]
async fn dropping_the_response_stops_the_stream(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = runtime();
    let (closed, client_closed) = oneshot::channel();
    let url = runtime.block_on(serve(sse_then(b"data: a\n\n", closed)));
    let (client, tab_a, _tab_b) = client_and_owners(cx, &runtime);

    let response = send_as(cx, &client, tab_a, &url).await;
    assert!(matches!(response.body, ResponseBody::EventStream(_)));
    drop(response);

    // The client still holds A's slot; it must not hold the stream alive through it.
    wait_for_close(&runtime, client_closed, "dropping the response");
    drop(client);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_event_after_the_request_timeout_still_arrives() {
    let (closed, _client_closed) = oneshot::channel();
    let url = serve(vec![
        Step::Write(SSE_HEAD.to_vec()),
        Step::Pause(Duration::from_millis(500)),
        Step::Write(b"data: late\n\n".to_vec()),
        Step::AwaitClientClose(closed),
    ])
    .await;
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());

    let response = transport
        .send(request(&url, Duration::from_millis(200)))
        .await
        .expect("the head arrives inside the timeout");
    let stream = event_stream_of(&response);

    let event = tokio::time::timeout(WAIT, stream.next())
        .await
        .expect("the late event arrives");
    match event {
        Some(SessionEvent::Received(message)) => {
            assert_eq!(message.payload, Payload::Inline(b"late".to_vec()));
        }
        other => panic!("the event after the timeout is received, got {other:?}"),
    }
}

/// A head announcing ten body bytes, five of them, then the rest a second later.
fn dripping_body() -> Vec<Step> {
    vec![
        Step::Write(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\n\
              content-length: 10\r\n\r\nfirst"
                .to_vec(),
        ),
        Step::Pause(Duration::from_secs(1)),
        Step::Write(b"later".to_vec()),
    ]
}

async fn assert_drip_times_out(transport: ReqwestTransport) {
    let url = serve(dripping_body()).await;
    let timeout = Duration::from_millis(200);

    let outcome = tokio::time::timeout(WAIT, transport.send(request(&url, timeout)))
        .await
        .expect("the send ends");

    match outcome {
        Err(TransportError::Timeout { after }) => assert_eq!(after, timeout),
        other => panic!("a body dripping past the timeout fails with Timeout, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_in_memory_body_dripping_past_the_timeout_fails_with_timeout() {
    assert_drip_times_out(ReqwestTransport::new(tokio::runtime::Handle::current())).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spilled_body_dripping_past_the_timeout_fails_with_timeout() {
    assert_drip_times_out(
        ReqwestTransport::new(tokio::runtime::Handle::current()).with_spill_limit(2),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_bytes_stay_bounded_with_a_slow_consumer() {
    const EVENTS: usize = 24;
    const EVENT_DATA: usize = 1024 * 1024;
    let mut event = b"data: ".to_vec();
    event.resize(6 + EVENT_DATA, b'x');
    event.extend_from_slice(b"\n\n");
    let mut steps = vec![Step::Write(SSE_HEAD.to_vec())];
    steps.extend((0..EVENTS).map(|_| Step::Write(event.clone())));
    let url = serve(steps).await;
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());

    let response = transport
        .send(request(&url, Duration::from_secs(30)))
        .await
        .expect("the head arrives");
    let stream = event_stream_of(&response);

    // Nobody reads for a while: the pump fills its budget, then stops reading the body.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let queued = stream.queued_bytes();
    assert!(queued > 0, "the pump queued events while nobody read");
    assert!(
        queued <= MAX_QUEUED_BYTES + EVENT_DATA,
        "{queued} bytes queued, over the {MAX_QUEUED_BYTES}-byte budget plus one event"
    );

    let mut received = 0;
    loop {
        let event = tokio::time::timeout(WAIT, stream.next())
            .await
            .expect("the next event arrives");
        match event {
            Some(SessionEvent::Received(_)) => received += 1,
            Some(SessionEvent::Closed { .. }) | None => break,
            Some(other) => panic!("only events and the close arrive, got {other:?}"),
        }
        assert!(stream.queued_bytes() <= MAX_QUEUED_BYTES + EVENT_DATA);
    }
    assert_eq!(
        received, EVENTS,
        "every event arrives once the reader catches up"
    );
    assert_eq!(stream.queued_bytes(), 0, "nothing stays counted once read");
}
