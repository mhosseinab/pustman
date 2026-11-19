//! The per-workspace cookie jar behind the Manage Cookies dialog (PRD REQ-HTTP-08):
//! what a response's `Set-Cookie` stored, which cookies go out with the next request to
//! a URL, and the `list`, `add`, `edit`, `delete` and clear-by-domain operations the
//! dialog needs. The jar is local to one workspace and never shared (REQ-WSP-06; its
//! persistence lands in S15b, its transport wiring in S15c).
//!
//! cookie_store does the RFC 6265 work — `Set-Cookie` parsing, domain and path matching,
//! expiry — so this module wraps it and re-implements none of it. Cookie values are
//! secrets (PRD §7): this module logs nothing at all, [`CookieEntry`]'s `Debug` redacts
//! the value, and [`CookieJar`]'s `Debug` prints counts only.

use std::cmp::Reverse;
use std::fmt;
use std::sync::LazyLock;

use cookie_store::{Cookie as StoreCookie, CookieExpiration, CookieStore, RawCookie};
use publicsuffix::List as PublicSuffixList;
use time::OffsetDateTime;
use url::Url;

#[cfg(test)]
#[path = "cookies_tests.rs"]
mod tests;

/// One cookie as the Manage Cookies dialog lists it (REQ-HTTP-08). The same shape goes
/// back into the jar through [`CookieJar::add`] and [`CookieJar::edit`].
#[derive(Clone, PartialEq, Eq)]
pub struct CookieEntry {
    /// The host the cookie is scoped to, exactly as the jar keys it — `list`'s output is
    /// the identity `edit` and `delete` take.
    pub domain: String,
    /// The RFC 6265 cookie-path the cookie is scoped to.
    pub path: String,
    /// The cookie's name.
    pub name: String,
    /// The cookie's value: a secret (PRD §7), so `Debug` prints `<redacted>`.
    pub value: String,
    /// When the cookie dies, or `None` for a session cookie.
    pub expires: Option<OffsetDateTime>,
    /// Whether the cookie goes out over HTTPS only.
    pub secure: bool,
    /// Whether the cookie is withheld from non-HTTP APIs.
    pub http_only: bool,
}

impl fmt::Debug for CookieEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The value is a secret (PRD §7), redacted exactly as `Header`'s `Debug`
        // redacts its value (`request.rs`).
        f.debug_struct("CookieEntry")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("expires", &self.expires)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .finish()
    }
}

/// What a cookie operation refused. The dialog shows `Display`; both `Display` and
/// `Debug` name the cookie and never its value (PRD §7).
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CookieError {
    /// There is no cookie with this (domain, path, name) in the jar.
    #[error("No cookie named `{name}` for `{domain}` at `{path}`.")]
    UnknownCookie {
        domain: String,
        path: String,
        name: String,
    },

    /// The cookie cannot be held for the domain it names.
    #[error("Cookie `{name}` is not valid for `{domain}`: {reason}")]
    InvalidCookie {
        domain: String,
        name: String,
        /// Why, in cookie_store's words: its messages never carry a value.
        reason: String,
    },
}

/// The Mozilla Public Suffix List, vendored unedited from
/// <https://publicsuffix.org/list/public_suffix_list.dat> (MPL-2.0, pulled 2026-11-19).
/// It is what keeps a `Domain=com` cookie from becoming a supercookie for every `*.com`
/// host and a `Domain=co.uk` cookie from reaching sibling registrable domains.
const PUBLIC_SUFFIX_LIST_DATA: &str = include_str!("../assets/public_suffix_list.dat");

/// The parsed list, once for the process; every jar clones from it ([`Default`]).
/// `pub(crate)` for [`crate::cookie_persistence`], which re-checks reloaded rows against
/// it (a tampered table must not smuggle a public-suffix cookie past the jar).
pub(crate) static PUBLIC_SUFFIX_LIST: LazyLock<PublicSuffixList> = LazyLock::new(|| {
    // The data is compile-time constant, so a parse failure is a build defect. It fails
    // loudly here rather than silently handing out jars without public-suffix
    // rejection; `test_bundled_public_suffix_list_parses` pins the parse in tests.
    match PUBLIC_SUFFIX_LIST_DATA.parse::<PublicSuffixList>() {
        Ok(list) => list,
        Err(error) => {
            panic!("the vendored public suffix list does not parse (build defect): {error}")
        }
    }
});

/// The cookies of one workspace (REQ-HTTP-08). cookie_store holds them; this wrapper is
/// the jar's whole API. Every jar carries the vendored public suffix list: cookie_store
/// only rejects a public-suffix Domain when one is set (RFC 6265 §5.3 step 5).
pub struct CookieJar {
    store: CookieStore,
}

impl Default for CookieJar {
    fn default() -> Self {
        // `publicsuffix::List` clones deep (a trie of maps), and `CookieStore` owns its
        // list, so each jar pays one clone at creation — once per workspace, never per
        // request.
        Self {
            store: CookieStore::new_with_public_suffix(Some(PUBLIC_SUFFIX_LIST.clone())),
        }
    }
}

impl CookieJar {
    /// An empty jar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores the `Set-Cookie` headers of a response from `url` (REQ-HTTP-08).
    ///
    /// `Set-Cookie` is untrusted data: a header cookie_store cannot parse, one naming a
    /// name or value this jar could not put on a `Cookie` header unchanged, and one
    /// cookie_store rejects for `url` are dropped, never a panic and never a log line
    /// (RFC 6265 §5.2 lets a user agent drop the cookie). `CookieStore`'s own
    /// `store_response_cookies` is not used because it logs the whole cookie at debug
    /// level, and cookie values never reach logs (PRD §7).
    pub fn store_response_cookies(&mut self, url: &Url, set_cookie_headers: &[String]) {
        for header in set_cookie_headers {
            match RawCookie::parse(header) {
                Ok(cookie) if is_cookie_name(cookie.name()) && is_cookie_value(cookie.value()) => {
                    match self.store.insert_raw(&cookie, url) {
                        // The stored cookie takes its place in the jar (RFC 6265 §5.3).
                        Ok(_stored) => {}
                        // Rejected for `url`: wrong domain, expired on arrival, or an
                        // HttpOnly cookie from a non-HTTP scheme. A browser drops it too.
                        Err(_dropped) => {}
                    }
                }
                // Unparsable, or unpresentable in a `Cookie` header.
                _ => {}
            }
        }
    }

    /// The `Cookie` header value for a request to `url`, or `None` when no cookie matches
    /// (REQ-HTTP-08). Pairs go out longest path first (RFC 6265 §5.4) with a total order
    /// behind that, so the header does not jitter between sends.
    pub fn cookie_header_for(&self, url: &Url) -> Option<String> {
        let mut matched = self.store.matches(url);
        matched.sort_by_cached_key(|cookie| {
            (
                Reverse(cookie.path.len()),
                String::from(&cookie.domain),
                String::from(&cookie.path),
                cookie.name().to_owned(),
            )
        });
        let header = matched
            .into_iter()
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ");
        if header.is_empty() {
            None
        } else {
            Some(header)
        }
    }

    /// Every unexpired cookie in the jar — the dialog's rows — sorted by (domain, path,
    /// name), because a `HashMap`'s order would make the rows jitter.
    pub fn list(&self) -> Vec<CookieEntry> {
        let mut entries: Vec<CookieEntry> =
            self.store.iter_unexpired().map(entry_from_cookie).collect();
        entries.sort_by(|left, right| {
            (&left.domain, &left.path, &left.name).cmp(&(&right.domain, &right.path, &right.name))
        });
        entries
    }

    /// Adds a cookie typed into the Manage Cookies dialog (REQ-HTTP-08). The entry's
    /// domain names the exact host the cookie is scoped to: adding never widens a cookie
    /// to sibling hosts.
    pub fn add(&mut self, entry: CookieEntry) -> Result<(), CookieError> {
        let (cookie, url) = cookie_from_entry(&entry)?;
        self.store
            .insert(cookie, &url)
            .map(|_stored| ())
            .map_err(|error| invalid_cookie(&entry, &error.to_string()))
    }

    /// Replaces the cookie the jar identifies by (domain, path, name) with `replacement`
    /// (REQ-HTTP-08). The identity is what [`CookieJar::list`] reported; a replacement
    /// may move the row. An invalid replacement, or an unknown identity, changes nothing.
    pub fn edit(
        &mut self,
        domain: &str,
        path: &str,
        name: &str,
        replacement: CookieEntry,
    ) -> Result<(), CookieError> {
        let (cookie, url) = cookie_from_entry(&replacement)?;
        if !self.store.contains_any(domain, path, name) {
            return Err(unknown_cookie(domain, path, name));
        }
        let moved =
            cookie_identity(&cookie) != (domain.to_owned(), path.to_owned(), name.to_owned());
        // The replacement goes in before the edited cookie goes out: a rejected insert
        // then leaves the row it edits untouched.
        self.store
            .insert(cookie, &url)
            .map_err(|error| invalid_cookie(&replacement, &error.to_string()))?;
        if moved {
            self.store.remove(domain, path, name);
        }
        Ok(())
    }

    /// Removes the cookie the jar identifies by (domain, path, name) (REQ-HTTP-08).
    pub fn delete(&mut self, domain: &str, path: &str, name: &str) -> Result<(), CookieError> {
        match self.store.remove(domain, path, name) {
            Some(_removed) => Ok(()),
            None => Err(unknown_cookie(domain, path, name)),
        }
    }

    /// The jar's unexpired cookies, for [`crate::cookie_persistence`]'s snapshot into
    /// `storage` rows (REQ-WSP-06). The stored cookie is exposed, not [`CookieEntry`],
    /// because it keeps the host-only and domain flags a reload must restore.
    pub(crate) fn cookies(&self) -> impl Iterator<Item = &StoreCookie<'static>> {
        self.store.iter_unexpired()
    }

    /// A jar around cookies [`crate::cookie_persistence`] reloaded from `storage`
    /// (REQ-WSP-06). cookie_store's own store, not [`CookieJar::add`]: re-adding a
    /// `Domain=` cookie would narrow it to a host-only one. `from_cookies` builds its
    /// store without a public suffix list, so the vendored list is attached here and
    /// every constructor path carries it.
    pub(crate) fn from_store(store: CookieStore) -> Self {
        Self {
            store: store.with_suffix_list(PUBLIC_SUFFIX_LIST.clone()),
        }
    }

    /// Removes every cookie the jar holds for `domain`, and only those (REQ-HTTP-08).
    /// Returns how many cookies were removed, for the dialog's confirmation.
    pub fn clear_domain(&mut self, domain: &str) -> usize {
        let keys: Vec<(String, String)> = self
            .store
            .iter_any()
            .filter(|cookie| String::from(&cookie.domain) == domain)
            .map(|cookie| (String::from(&cookie.path), cookie.name().to_owned()))
            .collect();
        let mut removed = 0;
        for (path, name) in keys {
            if self.store.remove(domain, &path, &name).is_some() {
                removed += 1;
            }
        }
        removed
    }
}

impl fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only: what is inside is every cookie's value (PRD §7).
        f.debug_struct("CookieJar")
            .field("cookies", &self.store.iter_unexpired().count())
            .finish()
    }
}

/// The store cookie `entry` describes, validated and scoped to the host `entry` names.
/// cookie_store keeps the RFC 6265 semantics; this only feeds it a well-formed cookie.
fn cookie_from_entry(entry: &CookieEntry) -> Result<(StoreCookie<'static>, Url), CookieError> {
    if !is_cookie_name(&entry.name) {
        return Err(invalid_cookie(entry, "the name is not a token"));
    }
    if !is_cookie_value(&entry.value) {
        return Err(invalid_cookie(
            entry,
            "the value carries `;` or control characters",
        ));
    }
    if entry
        .expires
        .is_some_and(|when| when <= OffsetDateTime::now_utc())
    {
        // cookie_store reads an expired cookie as "delete this one" (RFC 6265 §5.3
        // step 4), so a past expiry here would silently drop the cookie being edited.
        return Err(invalid_cookie(entry, "the expiry is in the past"));
    }
    let domain = entry.domain.trim();
    let path = normalize_path(entry.path.trim());
    let url = request_url(domain, &path).ok_or_else(|| {
        invalid_cookie(
            entry,
            "the domain is not a host name a cookie can be scoped to",
        )
    })?;

    // No Domain attribute on purpose: the cookie is host-only for `entry`'s domain, so
    // an edit can never widen a cookie to sibling hosts.
    let raw = RawCookie::build((entry.name.as_str(), entry.value.as_str()))
        .path(path)
        .secure(entry.secure)
        .http_only(entry.http_only)
        .build();
    let mut cookie = StoreCookie::try_from_raw_cookie(&raw, &url)
        .map_err(|error| invalid_cookie(entry, &error.to_string()))?
        .into_owned();
    cookie.expires = match entry.expires {
        Some(when) => CookieExpiration::from(when),
        None => CookieExpiration::SessionEnd,
    };
    Ok((cookie, url))
}

/// Where the store keys a cookie: `CookieStore` addresses cookies by (domain, path,
/// name) exactly (RFC 6265 §5.3).
fn cookie_identity(cookie: &StoreCookie<'_>) -> (String, String, String) {
    (
        String::from(&cookie.domain),
        String::from(&cookie.path),
        cookie.name().to_owned(),
    )
}

/// One dialog row from one stored cookie.
fn entry_from_cookie(cookie: &StoreCookie<'_>) -> CookieEntry {
    CookieEntry {
        domain: String::from(&cookie.domain),
        path: String::from(&cookie.path),
        name: cookie.name().to_owned(),
        value: cookie.value().to_owned(),
        expires: match &cookie.expires {
            CookieExpiration::AtUtc(when) => Some(*when),
            CookieExpiration::SessionEnd => None,
        },
        secure: cookie.secure().unwrap_or(false),
        http_only: cookie.http_only().unwrap_or(false),
    }
}

/// The URL a manually entered cookie is built against: https, its own domain and path.
/// cookie_store derives the cookie's scope from that request URL (RFC 6265 §5.3).
fn request_url(domain: &str, path: &str) -> Option<Url> {
    let url = Url::parse(&format!("https://{domain}{path}")).ok()?;
    // A domain that did not land in the URL's host position — `user@host`, `host:port`,
    // anything else the url crate rewrote — is not the domain the cookie would use.
    url.host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case(domain))
        .then_some(url)
}

/// A cookie path starts with `/`; an empty path means the root (RFC 6265 §5.1.4).
fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        String::from("/")
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

/// A cookie name is an RFC 6265 token (§4.1.1); anything else would break the pair in
/// the `Cookie` header this jar builds. cookie_store's builder validates nothing.
fn is_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            matches!(
                byte,
                b'!'
                    | b'#'..=b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'0'..=b'9'
                    | b'A'..=b'Z'
                    | b'^'..=b'z'
                    | b'|'
                    | b'~'
            )
        })
}

/// The value goes on the wire as `name=value`, so `;` (which starts the next pair) and
/// control bytes (which are header injection) are refused at both entry points.
fn is_cookie_value(value: &str) -> bool {
    !value
        .bytes()
        .any(|byte| byte == b';' || byte.is_ascii_control())
}

fn unknown_cookie(domain: &str, path: &str, name: &str) -> CookieError {
    CookieError::UnknownCookie {
        domain: domain.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
    }
}

fn invalid_cookie(entry: &CookieEntry, reason: &str) -> CookieError {
    CookieError::InvalidCookie {
        domain: entry.domain.clone(),
        name: entry.name.clone(),
        reason: reason.to_owned(),
    }
}
