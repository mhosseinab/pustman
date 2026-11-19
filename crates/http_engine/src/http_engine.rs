//! `http_engine` — sending a request through a [`Transport`], and the failures the PRD §8.6
//! taxonomy names (PRD §5.1, REQ-HTTP-12).
//!
//! A request reaches the wire as a [`ResolvedRequest`] (PRD §7 resolves `{{variables}}`
//! before this crate sees it) and comes back as a [`Response`] or a [`TransportError`].
//! 4xx and 5xx are responses, not errors: they render normally (§8.6).
//!
//! [`HttpClient`] is the GPUI entity that owns an in-flight send. It holds the task, so
//! dropping it is a deliberate act: [`HttpClient::cancel`] drops it, which aborts the
//! connection and emits [`SendEvent::Cancelled`] for the Console (REQ-HTTP-12). A send is
//! never retried.
//!
//! Tests script a `FakeTransport` (behind the `test-support` feature) instead of
//! touching the network; the production transport lands separately.

mod client;
mod error;
mod request;
mod response;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod transport;

pub use client::{HttpClient, SendEvent};
pub use error::TransportError;
pub use request::{Header, RequestBody, ResolvedRequest, SendSettings};
pub use response::{
    Certificate, NetworkInfo, RedirectHop, Response, ResponseBody, ResponseVersion, Timings,
    TlsInfo,
};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::FakeTransport;
pub use transport::Transport;

/// A URL as it may be printed: query, fragment and userinfo dropped, because they can
/// carry secret values. Used by `Debug` output only; error reports go through redaction
/// separately.
pub(crate) fn redacted_url(url: &url::Url) -> String {
    let mut redacted = url.clone();
    redacted.set_query(None);
    redacted.set_fragment(None);
    // Userinfo carries credentials exactly as the query does. `set_username` and
    // `set_password` refuse a URL with no host (or a `file:` one), which is exactly the
    // set of URLs that carry no userinfo to clear, so a refusal is already redacted.
    let userinfo_cleared = redacted.set_username("").and(redacted.set_password(None));
    debug_assert!(
        userinfo_cleared.is_ok() || !redacted.has_host() || redacted.scheme() == "file",
        "a URL with a host accepts an empty userinfo"
    );
    redacted.to_string()
}
