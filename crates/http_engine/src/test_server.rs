//! A tokio + hyper loopback server for transport tests (`test-support`).
//!
//! [`TestServer`] answers every request with the request body echoed back and records
//! what arrived, which is what the transport tests need to see on the wire. Its TLS
//! variant serves HTTPS from a `rcgen` self-signed certificate covering `localhost` and
//! `127.0.0.1`, so a test can put it against certificate verification (REQ-HTTP-10).
//! [`TestServer::start_redirects`] serves a redirect chain for the manual redirect
//! recording of REQ-NET-03, and [`TestServer::start_delayed`] answers late enough to
//! trip a request timeout.
//!
//! The server runs on the tokio runtime of whoever awaits [`TestServer::start`]; the
//! `ReqwestTransport` under test runs on the same one.

use std::convert::Infallible;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use url::Url;

use crate::request::{self, Header};

/// One request [`TestServer`] received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedRequest {
    /// The method as it arrived.
    pub method: String,
    /// Path and query as they arrived: `/users?limit=10`.
    pub path_and_query: String,
    /// Headers in arrival order, duplicates kept.
    pub headers: Vec<Header>,
    /// The body as it arrived.
    pub body: Vec<u8>,
}

/// What a server does with the body of a request, once it has recorded it.
#[derive(Clone)]
enum Behavior {
    /// Answer with the request body echoed back.
    Echo,
    /// Answer the prepared replies of a redirect chain, matched by path.
    Redirects(Vec<RedirectReply>),
    /// Wait `delay`, then echo the body back.
    Delay { delay: Duration },
}

/// One prepared answer of a [`Behavior::Redirects`] chain.
#[derive(Clone)]
struct RedirectReply {
    /// The path this reply answers: `/` for the first hop, `/hop/<n>` after that.
    path: String,
    status: StatusCode,
    /// Where the hop points. `None` on the reply that ends the chain.
    location: Option<HeaderValue>,
    body: Bytes,
}

/// The path hop `index` of a redirect chain answers on.
fn chain_path(index: usize) -> String {
    if index == 0 {
        "/".to_string()
    } else {
        format!("/hop/{index}")
    }
}

/// A loopback HTTP server a test can send to and then inspect.
///
/// Dropping it stops the accept loop; connections already being served finish on their
/// own.
pub struct TestServer {
    url: Url,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    accept_task: tokio::task::JoinHandle<()>,
    certificate_der: Option<Vec<u8>>,
}

impl TestServer {
    /// An HTTP server on `127.0.0.1` and an ephemeral port.
    pub async fn start() -> std::io::Result<Self> {
        Self::start_with(None, Behavior::Echo).await
    }

    /// An HTTPS server on `127.0.0.1` and an ephemeral port, certified by a self-signed
    /// certificate that no client trusts by default.
    pub async fn start_tls() -> std::io::Result<Self> {
        Self::start_with(Some(self_signed_tls()?), Behavior::Echo).await
    }

    /// A server that redirects once per status in `statuses`, in chain order: `/`
    /// answers the first status with `Location: /hop/1`, `/hop/1` the second, and so on
    /// until `/hop/<statuses.len()>` answers `200` with `done`. A chain of `n` statuses
    /// serves exactly `n` redirect responses (REQ-NET-03).
    pub async fn start_redirects(statuses: &[u16]) -> std::io::Result<Self> {
        let mut replies = Vec::with_capacity(statuses.len() + 1);
        for (index, &status) in statuses.iter().enumerate() {
            let status = StatusCode::from_u16(status).map_err(|error| {
                std::io::Error::other(format!("{status} is not a valid status code: {error}"))
            })?;
            let next = chain_path(index + 1);
            let location = HeaderValue::from_str(&next).map_err(|error| {
                std::io::Error::other(format!("`{next}` is not a valid Location: {error}"))
            })?;
            replies.push(RedirectReply {
                path: chain_path(index),
                status,
                location: Some(location),
                body: Bytes::new(),
            });
        }
        replies.push(RedirectReply {
            path: chain_path(statuses.len()),
            status: StatusCode::OK,
            location: None,
            body: Bytes::from_static(b"done"),
        });
        Self::start_with(None, Behavior::Redirects(replies)).await
    }

    /// A server that answers the request at `/` with one redirect to `location` — an
    /// absolute URL pointing wherever the test says, another server included: a hop
    /// with a different origin is what the redirect credential rules key on.
    pub async fn start_redirect_to(location: &Url) -> std::io::Result<Self> {
        let location = HeaderValue::from_str(location.as_str()).map_err(|error| {
            std::io::Error::other(format!("`{location}` is not a valid Location: {error}"))
        })?;
        let replies = vec![RedirectReply {
            path: "/".to_string(),
            status: StatusCode::FOUND,
            location: Some(location),
            body: Bytes::new(),
        }];
        Self::start_with(None, Behavior::Redirects(replies)).await
    }

    /// A server that waits `delay` before answering every request with its echo, for a
    /// test that gives up first.
    pub async fn start_delayed(delay: Duration) -> std::io::Result<Self> {
        Self::start_with(None, Behavior::Delay { delay }).await
    }

    /// Where to send requests: `http://127.0.0.1:<port>/`, or `https://` for the TLS
    /// variant.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The requests received so far, in arrival order.
    pub fn requests(&self) -> Vec<ReceivedRequest> {
        lock(&self.requests).clone()
    }

    /// The DER certificate the TLS variant serves, for a test that wants to trust it
    /// explicitly. `None` on the plain server.
    pub fn certificate_der(&self) -> Option<&[u8]> {
        self.certificate_der.as_deref()
    }

    async fn start_with(tls: Option<Tls>, behavior: Behavior) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let scheme = if tls.is_some() { "https" } else { "http" };
        let url = Url::parse(&format!("{scheme}://{address}/")).map_err(|error| {
            std::io::Error::other(format!("the bound address is not a URL: {error}"))
        })?;
        let certificate_der = tls.as_ref().map(|tls| tls.certificate_der.clone());

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
                let behavior = behavior.clone();
                let acceptor = tls.as_ref().map(|tls| tls.acceptor.clone());
                tokio::spawn(async move {
                    match acceptor {
                        Some(acceptor) => {
                            let stream = match acceptor.accept(stream).await {
                                Ok(stream) => stream,
                                // A client that never finished the handshake: nothing to serve.
                                Err(_handshake_error) => return,
                            };
                            serve_connection(stream, records, behavior).await;
                        }
                        None => serve_connection(stream, records, behavior).await,
                    }
                });
            }
        });

        Ok(Self {
            url,
            requests,
            accept_task,
            certificate_der,
        })
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

/// The TLS half of [`TestServer::start_tls`].
struct Tls {
    acceptor: tokio_rustls::TlsAcceptor,
    certificate_der: Vec<u8>,
}

/// A certificate no client trusts: `rcgen` signs it itself, for `localhost` and the
/// loopback address the server binds.
fn self_signed_tls() -> std::io::Result<Tls> {
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .map_err(|error| {
                std::io::Error::other(format!(
                    "could not generate a self-signed certificate: {error}"
                ))
            })?;
    let certificate_der = certified.cert.der().to_vec();
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certified.cert.der().clone()], key)
        .map_err(|error| {
            std::io::Error::other(format!("could not build the TLS server config: {error}"))
        })?;
    Ok(Tls {
        acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
        certificate_der,
    })
}

/// Serve one connection until the peer goes away. Any error is the peer's: a client
/// that hangs up mid-request is normal in tests.
async fn serve_connection<IO>(
    io: IO,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    behavior: Behavior,
) where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let requests = requests.clone();
        let behavior = behavior.clone();
        async move { handle(request, requests, behavior).await }
    });
    if let Err(_serve_error) = http1::Builder::new()
        .serve_connection(TokioIo::new(io), service)
        .await
    {
        // Nothing to report: the connection just did not end cleanly.
    }
}

/// Record the request, then answer it the way the server was built to.
async fn handle(
    request: Request<Incoming>,
    requests: Arc<Mutex<Vec<ReceivedRequest>>>,
    behavior: Behavior,
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

    Ok(match behavior {
        Behavior::Delay { delay } => {
            tokio::time::sleep(delay).await;
            echo_response(body)
        }
        Behavior::Echo => echo_response(body),
        Behavior::Redirects(replies) => {
            match replies.iter().find(|reply| reply.path == path_and_query) {
                Some(reply) => chain_response(reply),
                None => not_found(),
            }
        }
    })
}

/// The answer [`Behavior::Echo`] gives: the request body back, so a test sees both
/// directions.
fn echo_response(body: Bytes) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body));
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/octet-stream"),
    );
    response
}

/// One prepared answer of a redirect chain: its status, its `Location`, its body.
fn chain_response(reply: &RedirectReply) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(reply.body.clone()));
    *response.status_mut() = reply.status;
    if let Some(location) = &reply.location {
        response.headers_mut().insert("location", location.clone());
    }
    response
}

/// A path outside the chain: no test should hit it, and one that does sees the mistake.
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
