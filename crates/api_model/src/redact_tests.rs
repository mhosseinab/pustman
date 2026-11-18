use crate::{SecretValues, redact_headers, redact_text, redact_url};

#[test]
fn redacts_secret_in_text() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    assert_eq!(
        redact_text("Authorization: Bearer token-REDACTED-example", &secrets),
        "Authorization: Bearer ••••••"
    );
    assert_eq!(
        redact_text(
            "token-REDACTED-example then token-REDACTED-example again",
            &secrets
        ),
        "•••••• then •••••• again"
    );
    assert_eq!(
        redact_text("nothing to hide here", &secrets),
        "nothing to hide here"
    );

    // A secret that contains a shorter secret must be masked whole: replacing the short one
    // first would leave the longer one's tail in the clear.
    let nested = SecretValues::from_values(["key1", "key1-verylongsecret"]);
    assert_eq!(
        redact_text("Authorization: key1-verylongsecret", &nested),
        "Authorization: ••••••",
        "a secret that contains a shorter secret is masked whole"
    );
    assert_eq!(
        redact_text("key1", &nested),
        "••••••",
        "the short secret alone is still masked"
    );
    // Overlapping secrets mask the whole overlapped span, so neither leaks a fragment.
    let overlapped = SecretValues::from_values(["abcd", "cdef"]);
    assert_eq!(
        redact_text("xxabcdefxx", &overlapped),
        "xx••••••xx",
        "overlapping secrets never leave a fragment of either in the clear"
    );
}

#[test]
fn ignores_short_and_empty_secrets() {
    let secrets = SecretValues::from_values(["", "ab", "abc", "abcd"]);
    assert_eq!(
        secrets.len(),
        1,
        "empty and under-four-character values must not become secrets"
    );
    assert_eq!(redact_text("ab abc abcd", &secrets), "ab abc ••••••");
    assert!(SecretValues::new().is_empty());
    assert!(SecretValues::default().is_empty());

    // A refused value would leak everywhere unmasked, so refusal is reported, never silent.
    assert_eq!(
        secrets.refused_count(),
        2,
        "every non-empty value that is not kept is counted as refused"
    );
    let mut reported = SecretValues::new();
    assert!(reported.insert("abcd"), "a kept value reports true");
    assert!(!reported.insert("ab••cd"), "a refused value reports false");
    assert_eq!(reported.refused_count(), 1);

    // Debug is what a panic or a log line prints, so it must not carry the values either.
    let debug = format!("{secrets:?}");
    assert!(
        !debug.contains("abcd"),
        "Debug output must not contain a secret value"
    );
}

#[test]
fn masks_authorization_and_cookie_headers_always() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    let headers = vec![
        (
            "Authorization".to_string(),
            "Bearer token-REDACTED-example".to_string(),
        ),
        (
            "Proxy-Authorization".to_string(),
            "Basic password-REDACTED".to_string(),
        ),
        ("cookie".to_string(), "session=1".to_string()),
        ("Set-Cookie".to_string(), "session=1; HttpOnly".to_string()),
        (
            "X-Api-Key".to_string(),
            "key token-REDACTED-example".to_string(),
        ),
        ("Accept".to_string(), "application/json".to_string()),
    ];
    let redacted = redact_headers(&headers, &secrets);
    assert_eq!(redacted[0].1, "••••••", "Authorization is masked in full");
    assert_eq!(
        redacted[1].1, "••••••",
        "Proxy-Authorization is masked in full"
    );
    assert_eq!(
        redacted[2].1, "••••••",
        "header names match case-insensitively"
    );
    assert_eq!(redacted[3].1, "••••••", "Set-Cookie is masked in full");
    assert_eq!(
        redacted[4].1, "••••••",
        "credential-shaped headers are masked in full whatever they contain"
    );
    assert_eq!(
        redacted[5].1, "application/json",
        "names and values without a credential survive"
    );
    for (before, after) in headers.iter().zip(redacted.iter()) {
        assert_eq!(before.0, after.0, "header names are left as they were");
    }

    let empty = SecretValues::new();
    let redacted = redact_headers(&headers[..2], &empty);
    assert_eq!(
        redacted[0].1, "••••••",
        "credential headers are masked even with no secrets known"
    );
    assert_eq!(redacted[1].1, "••••••");

    // A value that is not a credential keeps only its secret masked.
    let partial = vec![(
        "X-Trace".to_string(),
        "key token-REDACTED-example tail".to_string(),
    )];
    let redacted = redact_headers(&partial, &secrets);
    assert_eq!(
        redacted[0].1, "key •••••• tail",
        "any other value keeps only its secret masked"
    );

    // A secret in a header name must not survive in the name.
    let named = vec![
        ("X-token-REDACTED-example".to_string(), "plain".to_string()),
        ("token-REDACTED-example".to_string(), "plain".to_string()),
    ];
    let redacted = redact_headers(&named, &secrets);
    assert_eq!(
        redacted[0].0, "X-••••••",
        "a secret in a header name is masked"
    );
    assert_eq!(redacted[1].0, "••••••");
    assert_eq!(redacted[0].1, "plain", "values without a secret survive");

    // A secret glued into a credential header's name must not turn off the always-mask:
    // the first pass masks the name and must still mask the unknown credential value.
    let glued = vec![
        (
            "Authorizationtoken-REDACTED-example".to_string(),
            "Bearer unknown-token-abcdef".to_string(),
        ),
        (
            "Cookietoken-REDACTED-example".to_string(),
            "session=leakme".to_string(),
        ),
        (
            "Ctoken-REDACTED-exampleookie".to_string(),
            "session=leakme".to_string(),
        ),
    ];
    let once = redact_headers(&glued, &secrets);
    for (index, (name, value)) in once.iter().enumerate() {
        assert_eq!(
            value, "••••••",
            "a credential value is masked on the first pass even with a secret in its name: {}",
            glued[index].0
        );
        assert!(!name.contains("token-REDACTED-example"));
    }
    assert_eq!(redact_headers(&once, &secrets), once);
    // A secret inside the credential name itself breaks it on the first pass; the raw name
    // still decides.
    let inside = SecretValues::from_values(["ooki"]);
    let cookie = vec![("Cookie".to_string(), "session=leakme".to_string())];
    let once = redact_headers(&cookie, &inside);
    assert_eq!(
        once[0].1, "••••••",
        "a secret inside the header name keeps the mask"
    );
    assert_eq!(redact_headers(&once, &inside), once);

    // A credential header name padded with whitespace is still that header.
    let padded = [
        (" Cookie", "session=1"),
        ("Authorization ", "Bearer x"),
        ("Set-Cookie\t", "session=1"),
        ("Cookie\u{a0}", "session=1"),
        // Invisible format characters are not White_Space, so trim() alone keeps them.
        ("Cookie\u{200b}", "session=1"),
        ("\u{200c}Cookie", "session=1"),
        ("Authorization\u{2060}", "Bearer x"),
        ("\u{feff}Cookie", "session=1"),
        ("Coo\u{ad}kie", "session=1"),
    ];
    let headers: Vec<(String, String)> = padded
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let redacted = redact_headers(&headers, &empty);
    for (index, (_, value)) in redacted.iter().enumerate() {
        assert_eq!(
            value,
            "••••••",
            "a padded credential header name is still masked in full: {}",
            padded[index].0.escape_debug()
        );
    }

    // The always-masked list covers the credential headers beyond the mandated four.
    let credentials = [
        ("X-Api-Key", "key1"),
        ("Api-Key", "key2"),
        ("X-Auth-Token", "token1"),
        ("X-Access-Token", "token2"),
        ("X-CSRF-Token", "csrf1"),
        ("X-Amz-Security-Token", "aws1"),
        ("X-Hub-Signature", "sha1=abc"),
        ("X-Hub-Signature-256", "sha256=abc"),
        ("WWW-Authenticate", "Basic realm=x"),
        ("Proxy-Authenticate", "Basic"),
    ];
    let headers: Vec<(String, String)> = credentials
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let redacted = redact_headers(&headers, &empty);
    for (index, (_, value)) in redacted.iter().enumerate() {
        assert_eq!(
            value, "••••••",
            "credential header is masked in full even with no secrets known: {}",
            credentials[index].0
        );
    }
}

#[test]
fn redacts_url_userinfo_and_query_secret() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    assert_eq!(
        redact_url(
            "https://alice:password-REDACTED@example.com/path?token=token-REDACTED-example&q=2#frag",
            &secrets
        ),
        "https://alice:••••••@example.com/path?token=••••••&q=2#frag"
    );
    assert_eq!(
        redact_url(
            "https://bob:password-REDACTED@example.com/x",
            &SecretValues::new()
        ),
        "https://bob:••••••@example.com/x",
        "the userinfo password is masked even with no secrets known"
    );
    let encoded = SecretValues::from_values(["token-REDACTED@example"]);
    assert_eq!(
        redact_url(
            "https://example.com/cb?token=token-REDACTED%40example",
            &encoded
        ),
        "https://example.com/cb?token=••••••",
        "a percent-encoded secret names the same query value"
    );
    assert_eq!(
        redact_url("https://example.com/x", &secrets),
        "https://example.com/x"
    );

    // A secret that only appears percent-encoded must not survive outside query values
    // either: decoded text cannot be mapped back, so the component is masked whole.
    let encoded_secret = SecretValues::from_values(["tok3n+/x="]);
    assert_eq!(
        redact_url("https://example.com/dl/tok3n%2B%2Fx%3D", &encoded_secret),
        "https://example.com/dl/••••••",
        "a percent-encoded secret masks its whole path segment"
    );
    assert_eq!(
        redact_url("https://example.com/dl/x#tok3n%2B%2Fx%3D", &encoded_secret),
        "https://example.com/dl/x#••••••",
        "a percent-encoded secret masks the whole fragment"
    );
    assert_eq!(
        redact_url(
            "https://example.com/dl/x?tok3n%2B%2Fx%3D=1",
            &encoded_secret
        ),
        "https://example.com/dl/x?••••••=1",
        "a percent-encoded secret masks the whole query name"
    );
    assert_eq!(
        redact_url("https://tok3n%2B%2Fx%3D:pw@example.com/x", &encoded_secret),
        "https://••••••@example.com/x",
        "a percent-encoded secret masks the whole userinfo"
    );

    // A username-only userinfo is the credential itself (a PAT in a URL) and is masked
    // whole whatever the caller knows.
    assert_eq!(
        redact_url("https://ghp_abc123@github.com/x", &SecretValues::new()),
        "https://••••••@github.com/x",
        "a username-only userinfo is masked whole"
    );

    // Scheme-relative and schemeless URLs carry userinfo too.
    assert_eq!(
        redact_url("//user:pass@host/x", &SecretValues::new()),
        "//user:••••••@host/x",
        "a scheme-relative URL masks the userinfo password"
    );
    assert_eq!(
        redact_url("user:pass@host/x", &SecretValues::new()),
        "user:••••••@host/x",
        "a schemeless URL masks the userinfo password"
    );
    // A `://` later in the URL is not a scheme and must not hide the schemeless userinfo.
    for (url, expected) in [
        (
            "user:pass@host/callback?next=https://other",
            "user:••••••@host/callback?next=https://other",
        ),
        (
            "user:pass@host/db#see-https://docs",
            "user:••••••@host/db#see-https://docs",
        ),
        ("user:pass@host/a://b", "user:••••••@host/a://b"),
    ] {
        assert_eq!(
            redact_url(url, &SecretValues::new()),
            expected,
            "a later `://` must not unmask a schemeless userinfo: {url}"
        );
    }
    // A `://` before the `@` marks the authority even when the prefix is no valid scheme.
    for (url, expected) in [
        ("://user:pass@h/x", "://user:••••••@h/x"),
        ("1://user:pass@h/x", "1://user:••••••@h/x"),
        ("my_app://user:pass@h/x", "my_app://user:••••••@h/x"),
    ] {
        assert_eq!(
            redact_url(url, &SecretValues::new()),
            expected,
            "a malformed scheme must not unmask the userinfo: {url}"
        );
    }
    // A literal space in a `user:pass` userinfo is malformed, not a reason to leak it.
    for (url, expected) in [
        ("user:pass word@x/x", "user:••••••@x/x"),
        ("user name:pass@host/x", "user name:••••••@host/x"),
    ] {
        assert_eq!(
            redact_url(url, &SecretValues::new()),
            expected,
            "whitespace in a user:pass userinfo must not unmask it: {url}"
        );
    }
    assert_eq!(
        redact_url("contact bob@example.com", &SecretValues::new()),
        "contact bob@example.com",
        "prose around an email address has no userinfo"
    );

    // A secret holding a URL delimiter can straddle components; one encoded byte must not
    // let it through.
    for (secret, url, expected) in [
        ("ab/cd", "https://e.com/x/%61b/cd", "https://e.com/x/••••••"),
        (
            "a&bcdef",
            "https://e.com/q?x=%61&bcdef",
            "https://e.com/q?x=••••••",
        ),
        (
            "x=yqrs",
            "https://e.com/q?%78=yqrs",
            "https://e.com/q?••••••",
        ),
        (
            "a#bcdef",
            "https://e.com/x?y=%61#bcdef",
            "https://e.com/x?y=••••••",
        ),
        (
            "p/q=rstu",
            "https://e.com/d/p/q%3Drstu",
            "https://e.com/d/••••••",
        ),
        (
            "ab+cde",
            "https://e.com/p/%61b+cde",
            "https://e.com/p/••••••",
        ),
        // A form-encoded `+` is a space in a query value.
        (
            "a b&cd",
            "https://e.com/q?x=%61+b&cd",
            "https://e.com/q?x=••••••",
        ),
        (
            "tok@en-XX",
            "https://tok%40en-XX.example.com/x",
            "https://••••••/x",
        ),
    ] {
        let straddling = SecretValues::from_values([secret]);
        assert_eq!(
            redact_url(url, &straddling),
            expected,
            "a partially encoded secret spanning components is masked: {url}"
        );
    }
    // An opaque path has no userinfo: its first word names the scheme.
    assert_eq!(
        redact_url("mailto:bob@example.com", &SecretValues::new()),
        "mailto:bob@example.com",
        "a mailto target is an address, not a password"
    );
    // A registered secret must be masked whole even when it contains its own `://`: the
    // userinfo heuristic must not overwrite part of the occurrence before the literal pass.
    for (secret, url, expected) in [
        ("pa://ss", "user:pa://ss@host/x", "user:••••••@host/x"),
        (
            "postgres://user:pass@db",
            "conn=postgres://user:pass@db",
            "conn=••••••",
        ),
        ("pa:ss://tt", "user:pa:ss://tt@host/x", "user:••••••@host/x"),
        ("user:pa://ss", "user:pa://ss@host/x", "••••••@host/x"),
        // A secret straddling the `@` keeps its host-side fragment masked too.
        ("pass@host", "user:pass@host/x", "user:••••••/x"),
        // A secret whose tail overlaps the password must not leave the password prefix in
        // the clear: masking the union of the secret and the password region keeps only the
        // username.
        ("d@host", "user:my-password@host/x", "user:••••••/x"),
        ("token@host", "user:my-secret-token@host/x", "user:••••••/x"),
        (
            "d@host",
            "https://e.com/cb?next=user:my-password@host/x",
            "https://e.com/cb?next=user:••••••/x",
        ),
    ] {
        let known = SecretValues::from_values([secret]);
        assert_eq!(
            redact_url(url, &known),
            expected,
            "a registered secret is masked whole: {url}"
        );
    }
    // The userinfo contract holds for a URL nested inside another URL: masking the top-level
    // authority only would leave the inner password in the clear.
    for (url, expected) in [
        (
            "https://e.com/cb?next=https://user:pass@h/x",
            "https://e.com/cb?next=https://user:••••••@h/x",
        ),
        (
            "https://e.com/x#https://user:pass@h/y",
            "https://e.com/x#https://user:••••••@h/y",
        ),
        (
            "https://e.com/p/https://user:pass@h/y",
            "https://e.com/p/https://user:••••••@h/y",
        ),
        (
            "https://e.com/cb?next=user:my-password@host/x",
            "https://e.com/cb?next=user:••••••@host/x",
        ),
        // A nested schemeless or scheme-relative authority is masked on the same terms.
        (
            "https://e.com/cb?next=user:pass@h/x",
            "https://e.com/cb?next=user:••••••@h/x",
        ),
        (
            "https://e.com/cb?next=//user:pass@h/x",
            "https://e.com/cb?next=//user:••••••@h/x",
        ),
        (
            "https://e.com/x#//user:pass@h/y",
            "https://e.com/x#//user:••••••@h/y",
        ),
        // A second schemeless authority later in the string is masked too.
        (
            "user:pass@host/p?q=1&user:pass2@host2/p",
            "user:••••••@host/p?q=1&user:••••••@host2/p",
        ),
    ] {
        assert_eq!(
            redact_url(url, &SecretValues::new()),
            expected,
            "a nested URL's userinfo is masked: {url}"
        );
    }
    // A malformed scheme still marks an authority, and a `://` after the `@` does not.
    for (url, expected) in [
        ("user:pa://ss@host/x", "user:pa://••••••@host/x"),
        ("user:pa:ss://tt@host/x", "user:pa:ss://••••••@host/x"),
        ("user:pass@host/a://b", "user:••••••@host/a://b"),
        // A userinfo runs to the LAST `@`: a password may hold one, and a username may be
        // `user@tenant` in an Azure-SQL style string. Masking must not stop at the first `@`.
        ("user:p@ssw0rd@host/x", "user:••••••@host/x"),
        ("user@name:pass@host/x", "user@name:••••••@host/x"),
        (
            "postgres://user@example.com:pass@db/x",
            "postgres://user@example.com:••••••@db/x",
        ),
    ] {
        assert_eq!(
            redact_url(url, &SecretValues::new()),
            expected,
            "the password is masked whatever the caller knows: {url}"
        );
    }
    // A bullet run in a password normalizes to the mask's length rather than growing; and an
    // empty password has nothing to hide, so it is not expanded to a mask that would only
    // assert a password exists.
    assert_eq!(
        redact_url("user:••••••••@h/x", &SecretValues::new()),
        "user:••••••@h/x",
        "a bullet run in a password settles on the mask"
    );
    assert_eq!(
        redact_url("user:@h/x", &SecretValues::new()),
        "user:@h/x",
        "an empty password is not invented"
    );
}

#[test]
fn redaction_is_idempotent() {
    let secrets = SecretValues::from_values(["token-REDACTED-example", "token-REDACTED@example"]);
    let text = "Bearer token-REDACTED-example sent to token-REDACTED@example";
    let once = redact_text(text, &secrets);
    assert_eq!(redact_text(&once, &secrets), once);
    assert_eq!(redact_text("••••••", &secrets), "••••••");

    let headers = vec![
        (
            "Authorization".to_string(),
            "Bearer token-REDACTED-example".to_string(),
        ),
        ("X-Trace".to_string(), "token-REDACTED-example".to_string()),
    ];
    let once = redact_headers(&headers, &secrets);
    assert_eq!(redact_headers(&once, &secrets), once);

    let url = "https://alice:password-REDACTED@example.com/p?token=token-REDACTED%40example";
    let once = redact_url(url, &secrets);
    assert_eq!(redact_url(&once, &secrets), once);

    // Masking a secret that spans the scheme's `//` changes how the next pass parses the
    // authority; the result must still settle.
    let scheme_spanning = SecretValues::from_values(["//bo"]);
    let once = redact_url("https://bob:pw@h/x", &scheme_spanning);
    assert_eq!(redact_url(&once, &scheme_spanning), once);

    let straddling = SecretValues::from_values(["ab/cd", "a&bcdef", "ab+cde", "tok@en-XX"]);
    for url in [
        "user:pass@host/callback?next=https://other",
        "https://e.com/x/%61b/cd",
        "https://e.com/q?x=%61&bcdef",
        "https://e.com/p/%61b+cde",
        "https://tok%40en-XX.example.com/x",
        "://user:pass@h/x",
        "1://user:pass@h/x",
        "my_app://user:pass@h/x",
        "user:pass word@x/x",
        "user name:pass@host/x",
    ] {
        let once = redact_url(url, &straddling);
        assert_eq!(redact_url(&once, &straddling), once, "idempotent: {url}");
    }

    // The mask itself must not be usable as a secret, or every pass grows the mask.
    let mask_like = SecretValues::from_values(["••••", "•••••", "••••••••", "ab••"]);
    assert_eq!(
        mask_like.len(),
        0,
        "a value the mask could produce is not a secret"
    );
    assert_eq!(
        mask_like.refused_count(),
        4,
        "a value refused for holding the mask character is reported"
    );
    let masked = "value •••••• tail";
    assert_eq!(redact_text(masked, &mask_like), masked);
    assert_eq!(
        redact_text(&redact_text(masked, &mask_like), &mask_like),
        masked
    );
    let mixed = SecretValues::from_values(["token-REDACTED-example", "••••"]);
    assert_eq!(mixed.len(), 1);
    let once = redact_text("token-REDACTED-example ••••••", &mixed);
    assert_eq!(once, "•••••• ••••••");
    assert_eq!(redact_text(&once, &mixed), once);

    // A secret that contains another settles after one pass too.
    let nested = SecretValues::from_values(["key1", "key1-verylongsecret"]);
    let once = redact_text("key1 then key1-verylongsecret", &nested);
    assert_eq!(once, "•••••• then ••••••");
    assert_eq!(redact_text(&once, &nested), once);
}
