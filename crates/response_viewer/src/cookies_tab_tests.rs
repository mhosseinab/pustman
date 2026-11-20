//! The Cookies tab lists a response's `Set-Cookie` headers with every attribute
//! (REQ-RESP-02): the tab exists to *show what was parsed*, so the test feeds a real
//! response through the same path [`crate::ResponseView`] drives
//! ([`CookiesTab::set_headers`]) and asserts every attribute reaches the cell that shows
//! it. A stubbed parser leaves the rows empty or default-filled and fails every assert
//! here.

use gpui::TestAppContext;
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{ABSENT, CookiesTab};

#[gpui::test]
fn cookies_tab_lists_attributes(cx: &mut TestAppContext) {
    cx.update(init_test);
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.headers = vec![
        Header::new(
            "Set-Cookie",
            "sid=token-REDACTED-example; SameSite=Strict; Max-Age=3600",
        ),
        // The name match is case-insensitive: the header field arrives lowercased here.
        Header::new(
            "set-cookie",
            "theme=dark; Expires=Wed, 21 Oct 2026 07:28:00 GMT; Domain=example.com; Path=/app; Secure; HttpOnly",
        ),
        Header::new("Set-Cookie", "=missing-name"),
    ];
    let (view, cx) = cx.add_window_view(|_, cx| CookiesTab::new(cx));
    view.update(cx, |view, cx| view.set_headers(&response.headers, cx));
    draw_window(cx);

    let rows = view.read_with(cx, |view, _| view.rows().to_vec());
    assert_eq!(
        rows.len(),
        3,
        "every Set-Cookie header gets its own row, parsed or not"
    );

    let session = rows.first().expect("the first Set-Cookie gets a row");
    assert_eq!(session.name, "sid");
    assert_eq!(session.value, "token-REDACTED-example");
    assert_eq!(session.same_site, "Strict");
    assert_eq!(session.max_age, "3600");
    assert_eq!(
        session.domain, ABSENT,
        "an absent attribute shows as absent"
    );
    assert_eq!(session.path, ABSENT);
    assert_eq!(session.expires, ABSENT);
    assert_eq!(session.secure, ABSENT);
    assert_eq!(session.http_only, ABSENT);

    let persistent = rows.get(1).expect("the second Set-Cookie gets a row");
    assert_eq!(persistent.name, "theme");
    assert_eq!(persistent.value, "dark");
    assert_eq!(persistent.domain, "example.com");
    assert_eq!(persistent.path, "/app");
    assert!(
        persistent.expires.contains("21 Oct 2026"),
        "the Expires date shows, got {:?}",
        persistent.expires
    );
    assert_eq!(persistent.max_age, ABSENT);
    assert_eq!(persistent.secure, "Yes");
    assert_eq!(persistent.http_only, "Yes");
    assert_eq!(persistent.same_site, ABSENT);

    // A header that will not parse is shown, not dropped (the tab's Decision).
    let unparseable = rows.get(2).expect("the broken header still gets a row");
    assert!(unparseable.unparseable, "the row is marked unparseable");
    assert_eq!(
        unparseable.name, "=missing-name",
        "the raw header text shows"
    );
    assert_eq!(unparseable.value, "unparseable");

    // Cookie values are secrets (PRD §7): Debug never carries one.
    let debug = format!("{session:?}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "Debug redacts the cookie value, got {debug}"
    );
}

/// An unparseable row's name holds the raw header text — which typically embeds the
/// cookie value — so its `Debug` must not carry that text across the redaction boundary
/// (PRD §7), even though the UI cell still shows it. (`foo bar=<token>` parses to name
/// `foo bar` under `cookie`'s MissingPair/EmptyName-only errors; these two headers are
/// the genuinely unparseable shapes whose raw text embeds a token.)
#[test]
fn unparseable_cookie_debug_never_carries_the_raw_header_text() {
    let rows = super::parse_set_cookies(&[
        Header::new("Set-Cookie", "foo bar supersecrettoken"),
        Header::new("Set-Cookie", "=supersecrettoken2"),
    ]);
    assert_eq!(rows.len(), 2, "every broken header still gets a row");
    for row in &rows {
        assert!(row.unparseable, "the row is marked unparseable");
        let debug = format!("{row:?}");
        assert!(
            !debug.contains("supersecrettoken"),
            "Debug redacts an unparseable row's raw text, got {debug}"
        );
    }
}

/// A cookie with neither Expires nor Max-Age dies with the session (RFC 6265); the tab
/// says "Session" rather than showing the attribute as absent.
#[test]
fn cookie_without_expiry_or_max_age_shows_session() {
    let rows = super::parse_set_cookies(&[Header::new("Set-Cookie", "sid=token; Path=/")]);
    let row = rows.first().expect("the header gets a row");
    assert_eq!(row.expires, "Session", "a session cookie says Session");
}
