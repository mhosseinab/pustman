//! What comes back from a send: the response itself, and what it took to get it
//! (PRD §8.1, §8.2).

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use url::Url;

use crate::request::Header;

/// The HTTP version a response arrived on (REQ-RESP-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseVersion {
    Http10,
    Http11,
    Http2,
    Http3,
}

/// When each phase of a send took (REQ-NET-01). A phase the transport cannot measure
/// stays `None` — never estimated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timings {
    /// Waiting for a connection and preparing the request.
    pub prepare: Option<Duration>,
    /// DNS lookup.
    pub dns: Option<Duration>,
    /// TCP connect.
    pub connect: Option<Duration>,
    /// TLS handshake.
    pub tls: Option<Duration>,
    /// Time to the first response byte.
    pub first_byte: Option<Duration>,
    /// Downloading the body.
    pub download: Option<Duration>,
    /// The whole exchange.
    pub total: Option<Duration>,
}

/// The network facts of a send (REQ-NET-02). Anything the transport cannot observe is
/// `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkInfo {
    /// The remote IP the request ended up on.
    pub remote_ip: Option<String>,
    /// The remote port.
    pub remote_port: Option<u16>,
    /// The local socket address (`ip:port`) the request went out from.
    pub local_address: Option<String>,
    /// The protocol ALPN negotiated, when there is TLS and the transport can report it.
    pub alpn: Option<String>,
    /// The TLS facts, when the send used TLS.
    pub tls: Option<TlsInfo>,
}

/// The TLS facts of a send (REQ-NET-02). A fact the transport cannot observe is `None`,
/// never guessed (ADR-0003): `reqwest` reports the protocol version and nothing else of
/// these, so the cipher suite and the chain stay `None` until a transport can read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsInfo {
    /// The TLS protocol version, when the transport can report it.
    pub protocol: Option<String>,
    /// The cipher suite, when the transport can report it.
    pub cipher: Option<String>,
    /// The peer certificate chain, leaf first, when the transport can report it.
    pub certificates: Option<Vec<Certificate>>,
    /// Whether certificate verification was on for this send (REQ-HTTP-10).
    pub verification_enabled: bool,
}

/// One certificate of the peer chain (REQ-NET-02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub subject: String,
    pub issuer: String,
    /// Validity start, as the transport reports it.
    pub valid_from: String,
    /// Validity end, as the transport reports it.
    pub valid_until: String,
    pub subject_alt_names: Vec<String>,
    pub sha256_fingerprint: String,
}

/// One hop of a redirect chain (REQ-NET-03).
#[derive(Clone, PartialEq)]
pub struct RedirectHop {
    /// The method this hop was sent with.
    pub method: api_model::Method,
    /// Where the hop was sent.
    pub url: Url,
    /// The status the hop answered with.
    pub status: u16,
    /// The hop's response headers, in order.
    pub headers: Vec<Header>,
    /// The `Location` this hop answered with: followed to the next hop, or the one the
    /// chain stopped at.
    pub location: Option<Url>,
    /// How long the hop took, when the transport measured it.
    pub timing: Option<Duration>,
}

impl fmt::Debug for RedirectHop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedirectHop")
            .field("method", &self.method)
            .field("url", &crate::redacted_url(&self.url))
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("location", &self.location.as_ref().map(crate::redacted_url))
            .field("timing", &self.timing)
            .finish()
    }
}

/// The body of a response (REQ-RESP-06).
#[derive(Clone, PartialEq, Eq)]
pub enum ResponseBody {
    /// The whole body in memory.
    Bytes(Vec<u8>),
    /// A body streamed to `path`, with `prefix` kept in memory for the preview and hex
    /// views.
    File { path: PathBuf, prefix: Vec<u8> },
}

impl ResponseBody {
    /// The bytes held in memory: all of them, or the prefix of a streamed body.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            ResponseBody::Bytes(bytes) => bytes,
            ResponseBody::File { prefix, .. } => prefix,
        }
    }
}

impl fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Bodies never reach logs (AGENTS.md › Security) and a body file's path can
        // carry a resolved secret (PRD §7): it is redacted exactly as
        // [`crate::error::TransportError::FileNotFound`] redacts one (`error.rs`).
        match self {
            ResponseBody::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            ResponseBody::File { prefix, .. } => f
                .debug_struct("File")
                .field("path", &"<redacted>")
                .field("prefix_bytes", &prefix.len())
                .finish(),
        }
    }
}

/// One HTTP response. 4xx and 5xx arrive as responses, not errors (PRD §8.6).
#[derive(Clone, PartialEq)]
pub struct Response {
    /// The status code. Every 4xx and 5xx renders normally.
    pub status: u16,
    /// The reason phrase, as it arrived.
    pub reason: String,
    /// The HTTP version the response arrived on.
    pub http_version: ResponseVersion,
    /// Response headers in order, duplicates kept (REQ-RESP-05).
    pub headers: Vec<Header>,
    pub body: ResponseBody,
    /// The phases the transport could measure (REQ-NET-01).
    pub timings: Timings,
    /// The connection facts behind the response (REQ-NET-02).
    pub network: Option<NetworkInfo>,
    /// The redirects that were followed to reach this response (REQ-NET-03).
    pub redirects: Vec<RedirectHop>,
}

impl Response {
    /// A response carrying `status`, `reason` and `body`. Everything a transport can only
    /// observe on the wire is left unmeasured here, and the transport fills it in.
    pub fn new(status: u16, reason: impl Into<String>, body: ResponseBody) -> Self {
        Self {
            status,
            reason: reason.into(),
            http_version: ResponseVersion::Http11,
            headers: Vec::new(),
            body,
            timings: Timings::default(),
            network: None,
            redirects: Vec::new(),
        }
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Response headers carry cookies and the body carries data: print neither.
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("reason", &self.reason)
            .field("http_version", &self.http_version)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("timings", &self.timings)
            .field("network", &self.network)
            .field("redirects", &self.redirects)
            .finish()
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
