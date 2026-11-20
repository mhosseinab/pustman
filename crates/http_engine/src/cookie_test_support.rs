//! The builders the cookie transport tests share (`cookie_transport_tests.rs`).
//!
//! `TestServer` (`test_server.rs`) records what arrived and answers echoes and prepared
//! redirect chains, but no answer of its carries a `Set-Cookie` header — and these tests
//! need responses that set cookies. This is that server: scripted answers by path,
//! `Set-Cookie` and `Location` included, recording what arrived as the same
//! [`ReceivedRequest`] rows `test_server.rs` defines (it is read-only for this step, so
//! the shape is reused and never re-declared). Nothing here runs as a test.

use std::convert::Infallible;
use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use url::Url;

use crate::error::TransportError;
use crate::request::{self, Header, RequestBody, ResolvedRequest, SendSettings};
use crate::test_server::ReceivedRequest;

/// One scripted answer, matched by the request's path and query.
#[derive(Clone)]
pub struct ScriptedReply {
    /// The path and query this answer serves: `/` or `/hop/1`.
    pub path: String,
    pub status: u16,
    /// The response headers, in order: a `Set-Cookie` or a `Location` among them.
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

/// A loopback HTTP server that answers the scripted replies it was started with and
/// records the requests that arrive. Dropping it stops the accept loop; connections
/// already being served finish on their own.
pub struct ScriptedServer {
    url: Url,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    accept_task: tokio::task::JoinHandle<()>,
}

impl ScriptedServer {
    /// A server on `127.0.0.1` and an ephemeral port, answering each reply at its path.
    pub async fn start(replies: Vec<ScriptedReply>) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let url = Url::parse(&format!("http://{address}/")).map_err(|error| {
            std::io::Error::other(format!("the bound address is not a URL: {error}"))
        })?;

        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                let (stream, _peer) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    // One refused or dropped connection is not the server's problem.
                    Err(_accept_error) => continue,
                };
                let records = records.clone();
                let replies = replies.clone();
                tokio::spawn(async move {
                    serve_connection(stream, records, replies).await;
                });
            }
        });

        Ok(Self {
            url,
            requests,
            accept_task,
        })
    }

    /// Where to send requests: `http://127.0.0.1:<port>/`.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The requests received so far, in arrival order.
    pub fn requests(&self) -> Vec<ReceivedRequest> {
        lock(&self.requests).clone()
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

/// One GET to `url` with default send settings: the request shape both tests send.
pub fn get_request(url: &Url) -> Result<ResolvedRequest, TransportError> {
    ResolvedRequest::new(
        api_model::Method::Get,
        url.as_str(),
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
}

/// Serve one connection until the peer goes away. Any error is the peer's: a client that
/// hangs up mid-request is normal in tests.
async fn serve_connection<IO>(
    io: IO,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    replies: Vec<ScriptedReply>,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let requests = requests.clone();
        let replies = replies.clone();
        async move { handle(request, requests, replies).await }
    });
    if let Err(_serve_error) = http1::Builder::new()
        .serve_connection(TokioIo::new(io), service)
        .await
    {
        // Nothing to report: the connection just did not end cleanly.
    }
}

/// Record the request, then answer it with the reply scripted for its path.
async fn handle(
    request: Request<Incoming>,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    replies: Vec<ScriptedReply>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = request.into_parts();
    let body = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_body_error) => Bytes::new(),
    };
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str().to_string())
        .unwrap_or_else(|| parts.uri.path().to_string());
    let received = ReceivedRequest {
        method: parts.method.to_string(),
        path_and_query: path_and_query.clone(),
        headers: request::headers_from_map(&parts.headers),
        body: body.to_vec(),
    };
    lock(&requests).push(received);

    let reply = replies.iter().find(|reply| reply.path == path_and_query);
    Ok(match reply {
        Some(reply) => scripted_response(reply),
        None => not_found(),
    })
}

/// The scripted answer: its status, its headers (a `Set-Cookie` or `Location` among
/// them) and its body. A header this script cannot put on the wire is skipped — the
/// test asserting what the CLIENT received fails visibly when one goes missing.
fn scripted_response(reply: &ScriptedReply) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(reply.body.clone())));
    *response.status_mut() =
        StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    for header in &reply.headers {
        if let Ok(name) = hyper::header::HeaderName::from_bytes(header.name.as_bytes())
            && let Ok(value) = HeaderValue::from_str(&header.value)
        {
            response.headers_mut().append(name, value);
        }
    }
    response
}

/// A path outside the script: no test should hit it, and one that does sees the mistake.
fn not_found() -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// The state lock. A poisoned lock still holds usable state for a test.
fn lock(requests: &Mutex<Vec<ReceivedRequest>>) -> MutexGuard<'_, Vec<ReceivedRequest>> {
    requests
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
