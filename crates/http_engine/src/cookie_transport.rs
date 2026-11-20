//! The cookie jar's part of one send: the `Cookie` header each hop goes out with, and
//! what each hop's `Set-Cookie` stores (REQ-HTTP-08, REQ-NET-04).
//!
//! [`crate::ReqwestTransport`] follows redirects by hand (REQ-NET-03), so cookies are
//! applied and captured by hand too — once per hop, against that hop's URL. reqwest's
//! own cookie store stays off: it would attach the header invisibly, while the header
//! actually sent must be part of the request record (REQ-NET-04).
//!
//! The jar does all cookie semantics — `Set-Cookie` parsing, domain and path matching,
//! expiry (`cookies.rs`) — and this module only moves values between the hop's header
//! list and the jar. Cookie values are secrets (PRD §7): nothing here logs, and the
//! applied header is a plain [`Header`], whose `Debug` redacts its value like every
//! other header's.

use std::sync::{Arc, Mutex, MutexGuard};

use reqwest::header::{COOKIE, SET_COOKIE};
use url::Url;

use crate::cookies::CookieJar;
use crate::request::Header;

#[cfg(test)]
#[path = "cookie_transport_tests.rs"]
mod tests;

/// The `Cookie` header of one hop, applied to `headers` — the resolved request's own
/// header list — so the header that goes on the wire is recorded in the request record
/// (REQ-NET-04). Returns the header it applied, so the next hop replaces it instead of
/// stacking a second `Cookie` header (RFC 6265 §5.4), or `None` when nothing applied.
///
/// The jar handle is the shape `cookie_persistence` loads and saves (`Arc<Mutex<_>>`);
/// the lock is held for one jar call and never across an await (AGENTS.md › Rules ›
/// Async).
///
/// A `Cookie` header the request supplied itself is kept exactly as the user wrote it
/// and the jar adds nothing alongside it: a user header of the same name wins, the rule
/// the auto headers follow too (`reqwest_transport`'s `wire_auto_headers`). It lives in
/// `headers` like every other request header, so a redirect that drops it cross-origin
/// drops it for good — the jar then applies whatever matches the new host.
pub(crate) fn apply_cookie_header(
    headers: &mut Vec<Header>,
    previously_applied: Option<Header>,
    url: &Url,
    jar: &Arc<Mutex<CookieJar>>,
) -> Option<Header> {
    if let Some(previous) = previously_applied
        && let Some(position) = headers.iter().position(|header| *header == previous)
    {
        headers.remove(position);
    }
    if headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(COOKIE.as_str()))
    {
        return None;
    }
    let value = lock(jar).cookie_header_for(url)?;
    let applied = Header::new(COOKIE.as_str(), value);
    headers.push(applied.clone());
    Some(applied)
}

/// Stores every `Set-Cookie` of a response from `url` into the jar (REQ-HTTP-08). The
/// jar parses and scopes each cookie (`cookies.rs`); this only collects the values, and
/// a response carrying no `Set-Cookie` does not touch the jar at all.
pub(crate) fn capture_cookies(jar: &Arc<Mutex<CookieJar>>, url: &Url, headers: &[Header]) {
    let set_cookie_headers: Vec<String> = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(SET_COOKIE.as_str()))
        .map(|header| header.value.clone())
        .collect();
    if set_cookie_headers.is_empty() {
        return;
    }
    lock(jar).store_response_cookies(url, &set_cookie_headers);
}

/// The jar lock. A poisoned lock still holds usable state, the rule the client cache
/// follows (`reqwest_transport`).
fn lock(jar: &Arc<Mutex<CookieJar>>) -> MutexGuard<'_, CookieJar> {
    jar.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
