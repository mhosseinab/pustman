//! `http_engine` — sending a request through a [`Transport`], and the failures the PRD §8.6
//! taxonomy names (PRD §5.1, REQ-HTTP-12).
//!
//! A request reaches the wire as a [`ResolvedRequest`] (PRD §7 resolves `{{variables}}`
//! before this crate sees it) and comes back as a [`Response`] or a [`TransportError`].
//! 4xx and 5xx are responses, not errors: they render normally (§8.6).
//! [`HttpClient::send_request`] takes the unresolved request instead and runs [`prepare`]
//! first: the secrets it references are read from the keychain and an unset one fails the
//! send before the transport is called (REQ-VAR-SHARE-02).
//!
//! [`HttpClient`] is the GPUI entity that owns an in-flight send. It holds the task, so
//! dropping it is a deliberate act: [`HttpClient::cancel`] drops it, which aborts the
//! connection and emits [`SendEvent::Cancelled`] for the Console (REQ-HTTP-12). A send is
//! never retried.
//!
//! Tests script a `FakeTransport` instead of touching the network, and run against a
//! loopback [`TestServer`] where they need a real socket (both behind the
//! `test-support` feature). What reaches a real server is [`ReqwestTransport`].

mod body;
mod client;
mod cookie_persistence;
mod cookie_transport;
mod cookies;
mod digest;
mod error;
pub mod event_source;
mod file_ref;
mod history;
mod pipeline;
mod prepare;
mod request;
mod request_auth;
mod reqwest_transport;
mod response;
mod spill;
#[cfg(any(test, feature = "test-support"))]
mod test_server;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod transport;

pub use client::{
    HttpClient, SecretKeyLookup, SendEvent, SendIdentity, SendInput, SendOutcome, SendOwner,
};
pub use cookie_persistence::{load_jar, save_jar, save_jar_latest};
pub use cookies::{CookieEntry, CookieError, CookieJar};
pub use error::TransportError;
pub use file_ref::resolve_file_ref;
pub use history::{HISTORY_BODY_LIMIT, HistoryDraft, HistoryRecorder};
pub use pipeline::{PipelineError, Warning, resolve_request};
pub use prepare::{Prepared, prepare, presend_warnings};
pub use request::{
    Header, MultipartField, MultipartValue, RequestBody, ResolvedRequest, SendSettings,
};
pub use request_auth::{AppliedAuth, apply_auth};
pub use reqwest_transport::ReqwestTransport;
pub use response::{
    Certificate, NetworkInfo, RedirectHop, Response, ResponseBody, ResponseVersion, Timings,
    TlsInfo,
};
pub use spill::{DEFAULT_SPILL_LIMIT, SpillWriter, SpilledFile};
#[cfg(any(test, feature = "test-support"))]
pub use test_server::{ReceivedRequest, TestServer};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::FakeTransport;
pub use transport::Transport;

/// A URL as it may be printed: the scheme and nothing else. The resolved URL, its host
/// and its path carry secret values — a webhook token lives in the path (PRD §7) — and
/// [`crate::error::TransportError`]'s `Debug` redacts all of them (`error.rs`), so this
/// follows that rule. Used by `Debug` output only; the UI keeps the real URL through
/// [`crate::error::TransportError::user_message`]. [`crate::request::ResolvedRequest`]'s
/// `Debug` deviates deliberately — one
/// read-only test file pins its narrower shape there (see `debug_url` in `request.rs`).
pub(crate) fn redacted_url(url: &url::Url) -> String {
    format!("{}://<redacted>", url.scheme())
}

#[cfg(test)]
#[path = "body_encoding_tests.rs"]
mod body_encoding_tests;

#[cfg(test)]
#[path = "body_test_support.rs"]
mod body_test_support;

#[cfg(test)]
#[path = "cookie_test_support.rs"]
mod cookie_test_support;

#[cfg(test)]
#[path = "debug_redaction_tests.rs"]
mod debug_redaction_tests;

mod retention;

/// GraphQL over the HTTP engine (REQ-GQL-01).
pub mod graphql;

/// A `text/event-stream` response streamed as session events (REQ-SSE-01).
pub mod event_stream;
