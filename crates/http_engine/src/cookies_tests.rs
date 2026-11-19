//! The cookie jar behind the Manage Cookies dialog (REQ-HTTP-08): domain and path
//! scoping, the management operations, and the redaction cookie values live under
//! (PRD §7). Fixture values are obviously fake (`.agents/rules/tests.md`).

use publicsuffix::{List as PublicSuffixList, Psl};
use time::{Duration, OffsetDateTime};
use url::Url;

use crate::cookies::{CookieEntry, CookieError, CookieJar};

/// An obviously fake cookie value (`.agents/rules/tests.md`).
const VALUE: &str = "token-REDACTED-example";

fn url(text: &str) -> Url {
    Url::parse(text).expect("a valid URL")
}

fn entry(domain: &str, path: &str, name: &str, value: &str) -> CookieEntry {
    CookieEntry {
        domain: domain.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        value: value.to_owned(),
        expires: None,
        secure: false,
        http_only: false,
    }
}

/// A cookie captured from one host and path goes out for that host and path only: not
/// for a parent path, not for a sibling host, and not for a host that merely ends in
/// the same text (RFC 6265 §5.1.3).
#[test]
fn cookie_scoped_by_domain_and_path() {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/api/v1"),
        &[format!("sid={VALUE}; Path=/api")],
    );

    assert_eq!(
        jar.list(),
        vec![entry("example.com", "/api", "sid", VALUE)],
        "the cookie is stored host-only, under its path"
    );
    assert_eq!(
        jar.cookie_header_for(&url("https://example.com/api/v2")),
        Some(format!("sid={VALUE}"))
    );
    assert_eq!(jar.cookie_header_for(&url("https://example.com/")), None);
    assert_eq!(
        jar.cookie_header_for(&url("https://api.example.com/api/v2")),
        None
    );
    assert_eq!(
        jar.cookie_header_for(&url("https://evil-example.com/api/v2")),
        None
    );
}

/// Delete removes the cookie from the dialog's rows and from what goes out on the wire.
#[test]
fn delete_cookie_removes_it() {
    let mut jar = CookieJar::new();
    jar.add(entry("example.com", "/", "sid", VALUE))
        .expect("the cookie is valid");

    jar.delete("example.com", "/", "sid")
        .expect("the cookie is listed");

    assert_eq!(jar.list(), Vec::new());
    assert_eq!(jar.cookie_header_for(&url("https://example.com/")), None);
}

/// Clear by domain clears that domain, and only that domain.
#[test]
fn clear_domain_removes_only_that_domain() {
    let mut jar = CookieJar::new();
    for cookie in [
        entry("example.com", "/", "sid", VALUE),
        entry("example.com", "/api", "api", VALUE),
        entry("other.test", "/", "other", VALUE),
    ] {
        jar.add(cookie).expect("the cookie is valid");
    }

    assert_eq!(jar.clear_domain("example.com"), 2);

    assert_eq!(jar.list(), vec![entry("other.test", "/", "other", VALUE)]);
    assert_eq!(jar.cookie_header_for(&url("https://example.com/api")), None);
    assert_eq!(
        jar.cookie_header_for(&url("https://other.test/")),
        Some(format!("other={VALUE}"))
    );
}

/// Edit replaces the cookie identified by (domain, path, name) — the identity `list`
/// reports — and an unknown identity changes nothing.
#[test]
fn edit_replaces_the_cookie_identified_by_domain_path_and_name() {
    let mut jar = CookieJar::new();
    jar.add(entry("example.com", "/api", "sid", VALUE))
        .expect("the cookie is valid");

    jar.edit(
        "example.com",
        "/api",
        "sid",
        entry("example.com", "/api", "sid", "token-REPLACED-example"),
    )
    .expect("the cookie is listed");

    assert_eq!(
        jar.list(),
        vec![entry(
            "example.com",
            "/api",
            "sid",
            "token-REPLACED-example"
        )]
    );

    let missing = jar.edit(
        "example.com",
        "/api",
        "ghost",
        entry("example.com", "/api", "ghost", VALUE),
    );
    assert!(matches!(missing, Err(CookieError::UnknownCookie { .. })));
    assert_eq!(
        jar.list(),
        vec![entry(
            "example.com",
            "/api",
            "sid",
            "token-REPLACED-example"
        )],
        "a failed edit leaves the row it edits alone"
    );

    // A replacement may move the row: the old identity goes, the new one stays.
    jar.edit(
        "example.com",
        "/api",
        "sid",
        entry("example.com", "/api", "token", VALUE),
    )
    .expect("the cookie is listed");

    assert_eq!(
        jar.list(),
        vec![entry("example.com", "/api", "token", VALUE)]
    );
}

/// Rows come back in a stable order — (domain, path, name), never `HashMap` order — so
/// the dialog does not jitter between frames.
#[test]
fn list_is_sorted_by_domain_path_and_name() {
    let mut jar = CookieJar::new();
    for cookie in [
        entry("zulu.test", "/", "a", VALUE),
        entry("example.com", "/api", "sid", VALUE),
        entry("example.com", "/", "beta", VALUE),
        entry("example.com", "/", "alpha", VALUE),
    ] {
        jar.add(cookie).expect("the cookie is valid");
    }

    let identity: Vec<(String, String, String)> = jar
        .list()
        .into_iter()
        .map(|cookie| (cookie.domain, cookie.path, cookie.name))
        .collect();

    assert_eq!(
        identity,
        vec![
            ("example.com".to_owned(), "/".to_owned(), "alpha".to_owned()),
            ("example.com".to_owned(), "/".to_owned(), "beta".to_owned()),
            (
                "example.com".to_owned(),
                "/api".to_owned(),
                "sid".to_owned()
            ),
            ("zulu.test".to_owned(), "/".to_owned(), "a".to_owned()),
        ]
    );
}

/// Untrusted Set-Cookie: what cookie_store cannot parse or store is skipped, never a
/// panic (RFC 6265 §5.2 lets a user agent drop the cookie).
#[test]
fn malformed_set_cookie_headers_are_skipped_not_panics() {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[
            String::new(),                                // no name=value pair
            String::from("=missing-name"),                // empty name
            String::from("bad name=value"),               // the name is not a token
            String::from("sid=value; Domain=other.test"), // not for this host
            format!("good={VALUE}"),                      // the one cookie that stores
        ],
    );

    assert_eq!(jar.list(), vec![entry("example.com", "/", "good", VALUE)]);
}

/// The dialog's input is validated at the jar: what would not survive a `Cookie` header
/// is refused with a `CookieError`, and nothing half-stores.
#[test]
fn add_rejects_an_invalid_cookie_for_its_domain() {
    let mut jar = CookieJar::new();
    for bad in [
        entry("not a host", "/", "sid", VALUE),
        entry("example.com", "/", "", VALUE),
        entry("example.com", "/", "a;b", VALUE),
        entry("example.com", "/", "sid", "x; admin=true"),
    ] {
        assert!(matches!(
            jar.add(bad),
            Err(CookieError::InvalidCookie { .. })
        ));
    }
    let mut expired = entry("example.com", "/", "sid", VALUE);
    expired.expires = Some(OffsetDateTime::now_utc() - Duration::days(1));
    assert!(matches!(
        jar.add(expired),
        Err(CookieError::InvalidCookie { .. })
    ));

    assert_eq!(jar.list(), Vec::new());
}

/// A cookie value is a secret (PRD §7): it never reaches `Debug`, the jar's `Debug` is
/// counts only, and error text names the cookie, never its value.
#[test]
fn cookie_values_never_reach_debug_or_error_output() {
    let mut jar = CookieJar::new();
    jar.add(entry("example.com", "/", "sid", VALUE))
        .expect("the cookie is valid");

    let listed = format!("{:?}", jar.list());
    assert!(!listed.contains(VALUE), "value leaked: {listed}");
    assert!(listed.contains("<redacted>"));

    let jar_debug = format!("{jar:?}");
    assert!(!jar_debug.contains(VALUE), "value leaked: {jar_debug}");
    assert!(!jar_debug.contains("sid"), "names stay out: {jar_debug}");

    let error = jar
        .edit(
            "example.com",
            "/",
            "ghost",
            entry("example.com", "/", "ghost", VALUE),
        )
        .expect_err("the cookie is unknown");
    for shown in [error.to_string(), format!("{error:?}")] {
        assert!(!shown.contains(VALUE), "value leaked: {shown}");
    }
}

/// A `Set-Cookie` whose Domain attribute is a public suffix is dropped (RFC 6265 §5.3
/// step 5): `Domain=com` must never become a supercookie sent to every `*.com` host, and
/// a `Domain=co.uk` cookie must never reach a sibling registrable domain.
#[test]
fn test_cookie_for_a_public_suffix_is_rejected() {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://api.example.com/"),
        &[format!("super={VALUE}; Domain=com")],
    );
    jar.store_response_cookies(
        &url("https://example.co.uk/"),
        &[format!("super={VALUE}; Domain=co.uk")],
    );

    assert_eq!(
        jar.list(),
        Vec::new(),
        "a public-suffix cookie is not stored"
    );
    assert_eq!(jar.cookie_header_for(&url("https://other.com/")), None);
    assert_eq!(jar.cookie_header_for(&url("https://victim.co.uk/")), None);
}

/// A Domain attribute naming a registrable domain is kept and domain-matches its
/// siblings: `Domain=example.com` set by `https://api.example.com/` reaches
/// `https://www.example.com/`.
#[test]
fn test_cookie_for_a_registrable_domain_is_kept() {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://api.example.com/"),
        &[format!("sid={VALUE}; Domain=example.com")],
    );

    assert_eq!(
        jar.cookie_header_for(&url("https://www.example.com/")),
        Some(format!("sid={VALUE}"))
    );
}

/// The vendored Mozilla Public Suffix List parses with the crate the jar builds its
/// rejection list from: the data is compile-time constant, so a parse failure is a build
/// defect (`cookies.rs`) and this is the test that pins the data.
#[test]
fn test_bundled_public_suffix_list_parses() {
    let parsed = include_str!("../assets/public_suffix_list.dat")
        .parse::<PublicSuffixList>()
        .expect("the vendored public suffix list parses");
    assert!(
        parsed.suffix(b"co.uk").is_some(),
        "the list knows multi-label public suffixes"
    );
}
