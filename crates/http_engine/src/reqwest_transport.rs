//! [`ReqwestTransport`]: a send that reaches a real server (PRD §5.1, REQ-HTTP-09/10).
//!
//! This is the only module that touches tokio. `reqwest` needs a tokio runtime, so the
//! exchange runs as a task on the [`tokio::runtime::Handle`] captured at construction —
//! the handle [`gpui_tokio::Tokio::handle`] takes from the runtime `gpui_tokio::init`
//! creates in the app. [`gpui_tokio::Tokio::spawn`] is not used directly because it
//! needs an `AppContext`, which a [`Transport::send`] never sees; a captured handle is
//! the alternative the `pustman-http-engine` skill names for exactly this case.
//!
//! Dropping the future [`Transport::send`] returns aborts the request: the task is
//! cancelled from [`AbortOnDrop`], the way dropping a `gpui_tokio::Tokio::spawn` task
//! cancels its tokio work.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use api_model::HttpVersion;
use futures::FutureExt;
use futures::future::BoxFuture;
use reqwest::header::{
    AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, HeaderName, HeaderValue, LOCATION,
    PROXY_AUTHORIZATION,
};
use url::Url;

use crate::error::TransportError;
use crate::request::{self, Header, RequestBody, ResolvedRequest, SendSettings};
use crate::response::{
    NetworkInfo, RedirectHop, Response, ResponseBody, ResponseVersion, Timings, TlsInfo,
};
use crate::transport::Transport;

/// A transport that sends through a `reqwest` client on a tokio runtime.
///
/// A [`reqwest::Client`] pools connections and owns the TLS configuration, so the
/// settings one client can honour decide which client a send gets: one client per
/// settings key, built once and reused ([`client_for`]).
pub struct ReqwestTransport {
    /// The tokio runtime the sends run on.
    handle: tokio::runtime::Handle,
    clients: Arc<Mutex<HashMap<ClientKey, reqwest::Client>>>,
}

impl ReqwestTransport {
    /// A transport that runs its sends on `handle`.
    ///
    /// In the app that handle is [`gpui_tokio::Tokio::handle`]`(cx)`, after
    /// `gpui_tokio::init(cx)`.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Transport for ReqwestTransport {
    fn send(
        &self,
        request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        // The client is built inside the spawned task, never here: building one loads
        // the system root certificate store, and `HttpClient::send` calls this on the
        // GPUI foreground (AGENTS.md › Rules › GPUI). This part is allocation only.
        let clients = Arc::clone(&self.clients);
        let key = ClientKey::for_settings(&request.settings);
        let handle = self.handle.clone();
        async move {
            let task = handle.spawn(async move {
                let client = client_for(&clients, key)?;
                execute(client, request).await
            });
            match AbortOnDrop::new(task).join().await {
                Ok(outcome) => outcome,
                Err(join_error) if join_error.is_cancelled() => Err(TransportError::Cancelled),
                Err(join_error) => Err(TransportError::Io {
                    detail: interrupted_detail("The send was interrupted", &join_error),
                }),
            }
        }
        .boxed()
    }
}

/// The settings a [`reqwest::Client`] is built for (REQ-HTTP-09/10). Redirects are not
/// part of it: the client never follows them, so every send uses the same redirect
/// policy and [`execute`] walks the chain itself, hop by hop (REQ-NET-03).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ClientKey {
    timeout: Duration,
    http_version: HttpVersion,
    verify_tls: bool,
}

impl ClientKey {
    fn for_settings(settings: &SendSettings) -> Self {
        Self {
            timeout: settings.timeout,
            http_version: settings.http_version,
            verify_tls: settings.verify_tls,
        }
    }
}

/// The client for these settings, building it on first use. Runs on the tokio task
/// [`Transport::send`] spawns: building a client loads the system root certificate
/// store, and that file IO must stay off the thread driving the UI (AGENTS.md › Rules ›
/// GPUI).
fn client_for(
    clients: &Mutex<HashMap<ClientKey, reqwest::Client>>,
    key: ClientKey,
) -> Result<reqwest::Client, TransportError> {
    if let Some(client) = lock(clients).get(&key) {
        return Ok(client.clone());
    }
    let client = build_client(key)?;
    lock(clients).insert(key, client.clone());
    Ok(client)
}

/// Build the client one settings key asks for.
fn build_client(key: ClientKey) -> Result<reqwest::Client, TransportError> {
    // `Policy::none()` is what makes the redirect handling manual: the chain is walked
    // in `execute`, so every hop is recorded (REQ-NET-03). `tls_info(true)` attaches the
    // negotiated TLS version and the peer certificate to each response, which is all
    // `network_info` can read (REQ-NET-02, ADR-0003).
    let mut builder = reqwest::Client::builder()
        .timeout(key.timeout)
        .tls_info(true)
        .redirect(reqwest::redirect::Policy::none());
    builder = match key.http_version {
        HttpVersion::Auto => builder,
        HttpVersion::Http11 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
    };
    if !key.verify_tls {
        // REQ-HTTP-10: turning verification off is a per-request choice the UI flags,
        // and it covers the certificate and the host name it is issued for.
        builder = builder
            .tls_danger_accept_invalid_certs(true)
            .tls_danger_accept_invalid_hostnames(true);
    }
    // `Io` logs its `detail` as it finds it (`error.rs`), so the URL goes first here
    // too: reqwest's `Display` appends the resolved one (PRD §7).
    builder.build().map_err(|error| TransportError::Io {
        detail: format!("Could not build the HTTP client: {}", error.without_url()),
    })
}

/// One send, on the tokio runtime: walk the redirect chain by hand — one request per
/// hop, every hop recorded (REQ-NET-03) — then read the final body.
async fn execute(
    client: reqwest::Client,
    request: ResolvedRequest,
) -> Result<Response, TransportError> {
    let settings = request.settings;
    let mut method = request.method;
    let mut url = request.url;
    let mut headers = request.headers;
    let mut body = request.body;
    let mut chain: Vec<RedirectHop> = Vec::new();

    loop {
        let hop_started = Instant::now();
        let hop = exchange(&client, &method, &url, &headers, &body, &settings).await?;
        let timing = hop_started.elapsed();

        if !settings.follow_redirects {
            return Ok(final_response(hop, chain));
        }
        let Some(location) = redirect_target(hop.status, &hop.headers, &url) else {
            // Not a redirect this transport follows: the response stands (a 3xx without
            // a usable `Location` renders like any other response).
            return Ok(final_response(hop, chain));
        };

        // The hop is recorded before the limit is checked, so the chain of a
        // `TooManyRedirects` ends on the hop that could not be followed.
        chain.push(RedirectHop {
            method: method.clone(),
            url: url.clone(),
            status: hop.status.as_u16(),
            headers: hop.headers.clone(),
            location: Some(location.clone()),
            timing: Some(timing),
        });
        if chain.len() as u32 > settings.max_redirects {
            return Err(TransportError::TooManyRedirects { chain });
        }

        // REQ-HTTP-09 `follow_original_method`: 307/308 always resend the request as it
        // was, 303 always becomes GET, and 301/302 become GET only when the request did
        // not ask for its original method back. A body that leaves the method leaves the
        // request.
        let keep_method = match hop.status.as_u16() {
            307 | 308 => true,
            303 => false,
            _ => settings.follow_original_method,
        };
        if !keep_method {
            method = api_model::Method::Get;
            body = RequestBody::None;
            // A content length that survives into the body-less GET announces bytes
            // it never sends, and servers wait for them; the content type named a body
            // that is gone.
            headers.retain(|header| {
                !header_name_is_one_of(&header.name, &[CONTENT_LENGTH, CONTENT_TYPE])
            });
        }
        // A hop is a new request: its headers are filtered against its own URL before
        // it goes out.
        headers = headers_for_redirect(headers, &url, &location, settings.keep_authorization);
        url = location;
    }
}

/// One exchange on the wire: the response's status, version, headers and body, plus what
/// the transport could measure and observe of the connection (REQ-NET-01/02).
struct Exchange {
    status: reqwest::StatusCode,
    http_version: ResponseVersion,
    headers: Vec<Header>,
    body: Vec<u8>,
    timings: Timings,
    network: Option<NetworkInfo>,
}

/// The response that ends the send: the last exchange, carrying the chain that reached
/// it (REQ-NET-03).
fn final_response(hop: Exchange, chain: Vec<RedirectHop>) -> Response {
    let reason = hop
        .status
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    Response {
        status: hop.status.as_u16(),
        reason,
        http_version: hop.http_version,
        headers: hop.headers,
        body: ResponseBody::Bytes(hop.body),
        timings: hop.timings,
        network: hop.network,
        redirects: chain,
    }
}

/// One request and its response, body read in full. The timeout is the request's: a
/// hop is a request of its own and gets the whole of it.
///
/// The timing phases (REQ-NET-01) are measured here, around `reqwest`'s own futures: the
/// clock starts when the request is handed to `reqwest` and stops when the body is in
/// hand. Phases inside that window — DNS, TCP connect, the TLS handshake — `reqwest`
/// does not report (ADR-0003), so they stay `None`: they are never estimated.
async fn exchange(
    client: &reqwest::Client,
    method: &api_model::Method,
    url: &Url,
    headers: &[Header],
    body: &RequestBody,
    settings: &SendSettings,
) -> Result<Exchange, TransportError> {
    let mut builder = client.request(method_of(method)?, url.clone());
    for (position, header) in headers.iter().enumerate() {
        // Neither the name nor the value is echoed: variables resolve into header names
        // too (PRD §7) and `Io`'s `Debug` prints `detail` as it finds it (`error.rs`), so
        // the message says which header failed and nothing it held.
        let name =
            HeaderName::from_bytes(header.name.as_bytes()).map_err(|error| TransportError::Io {
                detail: format!("header #{} has an invalid name: {error}", position + 1),
            })?;
        let value = HeaderValue::from_str(&header.value).map_err(|error| TransportError::Io {
            detail: format!("header #{} has an invalid value: {error}", position + 1),
        })?;
        builder = builder.header(name, value);
    }
    if let Some(body) = body_of(body).await? {
        builder = builder.body(body);
    }

    let started = Instant::now();
    let response = builder
        .send()
        .await
        .map_err(|error| map_error(error, settings.timeout, url))?;
    let first_byte = started.elapsed();
    let status = response.status();
    let http_version = version_of(response.version());
    let headers = request::headers_from_map(response.headers());
    // The extensions carry the connection facts and go away with the response, so they
    // are read before the body consumes it.
    let network = network_info(&response, url, settings.verify_tls);
    let download_started = Instant::now();
    let body = response
        .bytes()
        .await
        .map_err(|error| map_error(error, settings.timeout, url))?;
    let download = download_started.elapsed();

    Ok(Exchange {
        status,
        http_version,
        headers,
        body: body.to_vec(),
        timings: Timings {
            first_byte: Some(first_byte),
            download: Some(download),
            total: Some(started.elapsed()),
            ..Timings::default()
        },
        network,
    })
}

/// The headers a redirect hop may carry over (REQ-HTTP-09). The Host belongs to the
/// hop's own URL, and `Authorization` crosses a hop only when the request asked for
/// that. A hop that changes scheme, host or port is a new ORIGIN: the cookies and the
/// proxy credentials stop at its door — the line reqwest's own redirect handling draws
/// (`remove_sensitive_headers`) — so one redirect cannot walk a credential to another
/// server or onto cleartext. `keep_authorization` is the one override, and only for
/// `Authorization`.
fn headers_for_redirect(
    mut headers: Vec<Header>,
    previous: &Url,
    next: &Url,
    keep_authorization: bool,
) -> Vec<Header> {
    // `Url::origin` is scheme, host and effective port — the boundary the redirect
    // credential rules key on.
    let cross_origin = previous.origin() != next.origin();
    headers.retain(|header| {
        if header_name_is_one_of(&header.name, &[HOST]) {
            return false;
        }
        if header_name_is_one_of(&header.name, &[AUTHORIZATION]) {
            return keep_authorization;
        }
        if cross_origin {
            return !header_name_is_one_of(&header.name, &[COOKIE, COOKIE2, PROXY_AUTHORIZATION]);
        }
        true
    });
    headers
}

/// `Cookie2`: [`reqwest::header`] carries a constant for every other name these rules
/// match but not this one (RFC 2965 is dead, the header still turns up).
const COOKIE2: HeaderName = HeaderName::from_static("cookie2");

/// Whether the header name — carrying the casing the user typed — is one of `names`.
/// Parsing yields a [`HeaderName`], whose equality is case-insensitive by construction
/// (RFC 9110 §5.1), and a name the wire format refuses never matches: [`exchange`]
/// rejects it before it can go out.
fn header_name_is_one_of(name: &str, names: &[HeaderName]) -> bool {
    HeaderName::from_bytes(name.as_bytes()).is_ok_and(|parsed| names.contains(&parsed))
}

/// Where a redirect points, when this one is worth following: a redirect status with a
/// `Location` that resolves against the hop's own URL (it may be relative).
fn redirect_target(status: reqwest::StatusCode, headers: &[Header], url: &Url) -> Option<Url> {
    if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    let location = headers
        .iter()
        .find(|header| header_name_is_one_of(&header.name, &[LOCATION]))?;
    url.join(location.value.trim()).ok()
}

/// The bytes to send. A file body is read on tokio's blocking pool — never on the
/// thread driving the UI — and a missing one is [`TransportError::FileNotFound`]
/// (REQ-HTTP-05).
async fn body_of(body: &RequestBody) -> Result<Option<Vec<u8>>, TransportError> {
    match body {
        RequestBody::None => Ok(None),
        RequestBody::Bytes(bytes) => Ok(Some(bytes.clone())),
        RequestBody::File(path) => {
            let path = path.clone();
            let read = tokio::task::spawn_blocking({
                let path = path.clone();
                move || std::fs::read(path)
            });
            match read.await {
                Ok(Ok(bytes)) => Ok(Some(bytes)),
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    Err(TransportError::FileNotFound {
                        path: path.display().to_string(),
                    })
                }
                // The path stays out of the message: `Io` logs its detail and a path can
                // carry secret values (PRD §7).
                Ok(Err(error)) => Err(TransportError::Io {
                    detail: format!("Could not read the body file: {error}"),
                }),
                Err(join_error) => Err(TransportError::Io {
                    detail: interrupted_detail(
                        "Reading the body file was interrupted",
                        &join_error,
                    ),
                }),
            }
        }
    }
}

/// The verb as `reqwest` sends it. A custom token is validated as RFC 7230 by
/// `api_model::Method`, so this only fails on a token `http::Method` rejects.
fn method_of(method: &api_model::Method) -> Result<reqwest::Method, TransportError> {
    reqwest::Method::from_bytes(method.as_str().as_bytes()).map_err(|error| TransportError::Io {
        detail: format!("`{method}` is not a valid HTTP method: {error}"),
    })
}

/// The HTTP version the response arrived on (REQ-RESP-01).
fn version_of(version: reqwest::Version) -> ResponseVersion {
    match version {
        reqwest::Version::HTTP_10 => ResponseVersion::Http10,
        reqwest::Version::HTTP_11 => ResponseVersion::Http11,
        reqwest::Version::HTTP_2 => ResponseVersion::Http2,
        reqwest::Version::HTTP_3 => ResponseVersion::Http3,
        _ => ResponseVersion::Http11,
    }
}

/// What the connection behind a response reported (REQ-NET-02): the socket addresses
/// from the `HttpInfo` extension and the TLS facts from the `TlsInfo` one. `None` when
/// nothing was reported at all.
///
/// ALPN, the cipher suite and the peer certificate chain stay `None`: `reqwest` reports
/// none of them (ADR-0003), and guessing them from the negotiated HTTP version or from
/// what a server usually picks would be an estimate, not an observation.
fn network_info(response: &reqwest::Response, url: &Url, verify_tls: bool) -> Option<NetworkInfo> {
    let remote = response.remote_addr();
    let local = response
        .extensions()
        .get::<hyper_util::client::legacy::connect::HttpInfo>()
        .map(|info| info.local_addr().to_string());
    let tls = if url.scheme() == "https" {
        let reported = response
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.version())
            .and_then(tls_version_name);
        Some(TlsInfo {
            protocol: reported,
            cipher: None,
            certificates: None,
            verification_enabled: verify_tls,
        })
    } else {
        None
    };
    let info = NetworkInfo {
        remote_ip: remote.map(|address| address.ip().to_string()),
        remote_port: remote.map(|address| address.port()),
        local_address: local,
        alpn: None,
        tls,
    };
    (info.remote_ip.is_some() || info.local_address.is_some() || info.tls.is_some()).then_some(info)
}

/// The name of a TLS version `reqwest` reported. Its four constants are the whole set of
/// versions it can name; anything else is left unnamed rather than guessed.
fn tls_version_name(version: reqwest::tls::Version) -> Option<String> {
    if version == reqwest::tls::Version::TLS_1_3 {
        Some("TLS 1.3".to_string())
    } else if version == reqwest::tls::Version::TLS_1_2 {
        Some("TLS 1.2".to_string())
    } else if version == reqwest::tls::Version::TLS_1_1 {
        Some("TLS 1.1".to_string())
    } else if version == reqwest::tls::Version::TLS_1_0 {
        Some("TLS 1.0".to_string())
    } else {
        None
    }
}

/// Why the send failed, on the PRD §8.6 taxonomy. 4xx and 5xx never arrive here: they
/// are [`Response`]s.
fn map_error(error: reqwest::Error, timeout: Duration, url: &Url) -> TransportError {
    // `reqwest`'s `Display` appends the resolved URL, query values and path secrets
    // included (PRD §7), and `Tls.reason`/`Io.detail` print unredacted in `Debug`
    // (`error.rs`): the URL is dropped before any text from this error is scanned or
    // stringified. The chain below carries no other URL, so every element the marker
    // scan sees is URL-free — a TLS error's wording can still name the requested HOST,
    // and `tls_error_text` strips that where the reason is built.
    let error = error.without_url();
    if error.is_timeout() {
        return TransportError::Timeout { after: timeout };
    }
    // A DNS failure is never a handshake failure, so its kind decides before the TLS
    // wording is looked for — otherwise a failure whose text mentions TLS is misfiled.
    if error.is_dns() {
        return TransportError::Dns { host: host_of(url) };
    }
    // A TLS handshake fails while connecting, so it arrives looking like a connect
    // error and `is_connect` cannot decide first: the TLS evidence in the chain does.
    // It is asked of URL-free text, so a marker word in the resolved URL is never
    // evidence of a handshake failure.
    if let Some(reason) = tls_failure_reason(&error) {
        return TransportError::Tls { reason };
    }
    if let Some(detail) = protocol_failure_detail(&error) {
        return TransportError::Protocol { detail };
    }
    if error.is_connect() {
        return TransportError::Connect {
            address: authority_of(url),
        };
    }
    // What is left — a request that could not be built, a body that failed, a
    // decompression that failed — has no bucket of its own in §8.6 and lands in `Io`.
    TransportError::Io {
        detail: error.to_string(),
    }
}

/// The reason text of a TLS failure, when that is what this error is.
///
/// `reqwest` has no `is_tls()`, so the answer is asked of the errors inside it: the
/// `rustls` error in the chain (which `tokio-rustls` wraps in an `io::Error`), and the
/// wording rustls reports certificate and handshake failures with.
fn tls_failure_reason(error: &reqwest::Error) -> Option<String> {
    error_chain(error).into_iter().find_map(tls_error_text)
}

fn tls_error_text(error: &(dyn std::error::Error + 'static)) -> Option<String> {
    if error.downcast_ref::<rustls::Error>().is_some() {
        return Some(tls_reason(&error.to_string()));
    }
    // A `reqwest::Error`'s `Display` appends the URL it was sent to (PRD §7), so it is
    // never marker-scanned or stringified into `Tls.reason`: the wording of a real
    // handshake failure lives in the errors below it in the chain.
    if error.downcast_ref::<reqwest::Error>().is_some() {
        return None;
    }
    let text = error.to_string();
    const MARKERS: [&str; 3] = [
        "invalid peer certificate",
        "certificate not valid for name",
        "handshake",
    ];
    MARKERS
        .iter()
        .any(|marker| text.contains(*marker))
        .then(|| tls_reason(&text))
}

/// The reason text with any name or platform context stripped — two wordings become
/// static ones and no text from either survives. The rustls wording of a certificate
/// issued for another name repeats the REQUESTED server name (`NotValidForNameContext`
/// names it and the certificate's). The `Other` payloads — what rustls-platform-verifier
/// wraps OS trust errors in on macOS and Windows (`CertificateError::Other`, rendered
/// through the `Other(OtherError(..))` `Debug` fallback) and the `other error:` of
/// `Error::Other` — are opaque platform text that can embed the requested name too.
/// The requested name is resolved text that can carry a secret (PRD §7) while
/// `Tls.reason` prints unredacted in `Debug` (`error.rs`): no name survives this
/// function. Every other reason — `UnknownIssuer` and the rest — is kept as reported;
/// none of them names a host.
fn tls_reason(text: &str) -> String {
    const NAME_MISMATCH: &str = "certificate not valid for name";
    const OTHER_PAYLOAD: &str = "Other(OtherError(";
    const OTHER_ERROR: &str = "other error: ";
    if text.contains(OTHER_PAYLOAD) || text.contains(OTHER_ERROR) {
        "the platform rejected the server certificate".to_string()
    } else if text.contains(NAME_MISMATCH) {
        "invalid peer certificate: certificate not valid for name".to_string()
    } else {
        text.to_string()
    }
}

/// The reason text of a response the HTTP layer could not make sense of — a malformed
/// response, or a version the peer does not speak (§8.6 `Protocol`).
fn protocol_failure_detail(error: &reqwest::Error) -> Option<String> {
    for candidate in error_chain(error) {
        if let Some(hyper_error) = candidate.downcast_ref::<hyper::Error>()
            && (hyper_error.is_parse()
                || hyper_error.is_parse_status()
                || hyper_error.is_parse_version_h2()
                || hyper_error.is_incomplete_message())
        {
            return Some(hyper_error.to_string());
        }
    }
    None
}

/// Every error reachable from `error`, including the one an `io::Error` wraps:
/// `io::Error::source` returns the source of the error it wraps, not the error itself.
fn error_chain(error: &reqwest::Error) -> Vec<&(dyn std::error::Error + 'static)> {
    let mut chain = Vec::new();
    let mut current = Some(error as &(dyn std::error::Error + 'static));
    while let Some(candidate) = current {
        chain.push(candidate);
        if let Some(io_error) = candidate.downcast_ref::<std::io::Error>()
            && let Some(wrapped) = io_error.get_ref()
        {
            chain.push(wrapped);
        }
        current = candidate.source();
    }
    chain
}

/// The host name a failure names (§8.6 `Dns`).
fn host_of(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_string()
}

/// The host and port a failure names (§8.6 `Connect`: "Connection refused at
/// 127.0.0.1:8080."). The userinfo is left out: it carries credentials.
fn authority_of(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// The `Io` detail of a task that did not run to completion. tokio's `JoinError`
/// `Display` for a panicked task is "task N panicked with message …" — the panic
/// payload, which can carry resolved text (PRD §7), while `Io` prints `detail`
/// unredacted (`error.rs`). So the payload is never flattened into the detail: the
/// message is static and the kind says how the task ended. Anything deeper belongs to
/// the task error's own `source()` chain — never to a string a `Debug` prints.
fn interrupted_detail(message: &str, error: &tokio::task::JoinError) -> String {
    let kind = if error.is_panic() {
        "the task panicked"
    } else if error.is_cancelled() {
        "the task was cancelled"
    } else {
        "the task failed"
    };
    format!("{message}: {kind}")
}

/// Cancels the tokio task when the future driving it is dropped, which is what
/// [`Transport::send`] promises of its future.
struct AbortOnDrop<T> {
    task: tokio::task::JoinHandle<T>,
}

impl<T> AbortOnDrop<T> {
    fn new(task: tokio::task::JoinHandle<T>) -> Self {
        Self { task }
    }

    /// Wait for the task, still cancelling it if this future goes away first.
    async fn join(mut self) -> Result<T, tokio::task::JoinError> {
        (&mut self.task).await
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The lock of the client cache. A poisoned lock still holds usable state.
fn lock(
    clients: &Mutex<HashMap<ClientKey, reqwest::Client>>,
) -> MutexGuard<'_, HashMap<ClientKey, reqwest::Client>> {
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[path = "reqwest_transport_tests.rs"]
mod tests;
