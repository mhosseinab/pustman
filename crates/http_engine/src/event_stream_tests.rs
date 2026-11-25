//! An event-stream response streams (REQ-SSE-01): its events arrive while the server
//! still holds the connection open, the server's close ends the stream with a `Closed`,
//! and a stop closes the TCP connection — observed from the server's side of the socket.
//!
//! [`TestServer`](crate::TestServer) answers with whole bodies only, so these tests serve
//! the event stream from a loopback socket of their own: the response head, then the
//! scripted event bytes, written as the test says.

use std::time::Duration;

use api_model::{CloseReason, Payload, SessionEvent};
use futures::channel::oneshot;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use super::EventStream;
use crate::ReqwestTransport;
use crate::{RequestBody, ResolvedRequest, ResponseBody, SendSettings, Transport};

/// Long enough for a loopback exchange on a loaded machine, short enough that a body the
/// transport buffers whole fails the test instead of hanging it.
const WAIT: Duration = Duration::from_secs(10);

/// What the loopback server does once its event bytes are written.
enum Then {
    /// Keep the connection open until the client closes it, then report that.
    WaitForClientClose(oneshot::Sender<()>),
    /// Close the connection: the server ends the stream.
    Close,
}

/// Serve one `text/event-stream` response on `127.0.0.1`: the head, then `events`, then
/// `then`. The body has no length, so it runs until one side closes the connection.
async fn serve_events(events: &'static [u8], then: Then) -> Url {
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
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream; charset=utf-8\r\n\
                  cache-control: no-cache\r\nconnection: close\r\n\r\n",
            )
            .await
            .expect("the response head");
        socket.write_all(events).await.expect("the events");
        socket.flush().await.expect("a flush");
        match then {
            Then::Close => drop(socket),
            Then::WaitForClientClose(closed) => {
                // A read of zero bytes (or a reset) is the client closing the connection.
                loop {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
                closed.send(()).expect("the test waits for the close");
            }
        }
    });
    Url::parse(&format!("http://{address}/events")).expect("a loopback URL")
}

/// Send a GET to `url` through the real transport and return the event stream its
/// response carries.
async fn open_stream(url: &Url) -> EventStream {
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        url.as_str(),
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a resolvable URL");
    let response = tokio::time::timeout(WAIT, transport.send(request))
        .await
        .expect("the response arrives while the stream is still open")
        .expect("a response");
    assert_eq!(response.status, 200);
    match response.body {
        ResponseBody::EventStream(stream) => stream,
        other => panic!("an event-stream response carries an event stream, got {other:?}"),
    }
}

async fn next_event(stream: &EventStream) -> Option<SessionEvent> {
    tokio::time::timeout(WAIT, stream.next())
        .await
        .expect("the next event arrives in time")
}

fn received_data(event: Option<SessionEvent>) -> Vec<u8> {
    match event {
        Some(SessionEvent::Received(message)) => match message.payload {
            Payload::Inline(bytes) => bytes,
            other => panic!("an event's data is inline, got {other:?}"),
        },
        other => panic!("expected a received event, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_arrive_before_the_stream_ends() {
    let (closed, client_closed) = oneshot::channel();
    let url = serve_events(
        b"data: first\n\nevent: update\ndata: second\n\n",
        Then::WaitForClientClose(closed),
    )
    .await;

    let stream = open_stream(&url).await;

    assert_eq!(received_data(next_event(&stream).await), b"first");
    assert_eq!(received_data(next_event(&stream).await), b"second");
    // The server still holds the connection open: both events came while it streamed.
    drop(stream);
    tokio::time::timeout(WAIT, client_closed)
        .await
        .expect("dropping the stream closes the connection")
        .expect("the server reports the close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_close_emits_closed() {
    let url = serve_events(
        b"data: only\n\ndata: cut off without its blank line",
        Then::Close,
    )
    .await;

    let stream = open_stream(&url).await;

    assert_eq!(received_data(next_event(&stream).await), b"only");
    assert_eq!(
        next_event(&stream).await,
        Some(SessionEvent::Closed {
            reason: CloseReason::Server {
                code: None,
                reason: String::new(),
            },
        }),
        "the server's close ends the stream, and the unfinished event is not dispatched"
    );
    assert_eq!(next_event(&stream).await, None, "nothing follows the close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_closes_the_connection() {
    let (closed, client_closed) = oneshot::channel();
    let url = serve_events(b"data: before stop\n\n", Then::WaitForClientClose(closed)).await;

    let stream = open_stream(&url).await;
    assert_eq!(received_data(next_event(&stream).await), b"before stop");

    stream.stop();

    tokio::time::timeout(WAIT, client_closed)
        .await
        .expect("stopping the stream closes the TCP connection")
        .expect("the server reports the close");
    assert!(stream.is_stopped());
    assert_eq!(
        next_event(&stream).await,
        Some(SessionEvent::Closed {
            reason: CloseReason::Client,
        }),
        "a stop ends the stream with a client close"
    );
    assert_eq!(next_event(&stream).await, None, "nothing follows the close");
}
