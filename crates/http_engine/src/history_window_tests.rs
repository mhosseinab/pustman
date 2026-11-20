//! The body window's two edges (REQ-HIST-02): a read window cut mid-character inside a
//! secret must not store the secret's ASCII prefix, a body the window holds whole must be
//! stored whole even when its ending looks like a secret prefix, and a secret straddling
//! the window end of a longer body is still trimmed. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use api_model::SecretValues;

use super::{HISTORY_BODY_LIMIT, body_window_and_size, redacted_body_prefix};
use crate::ResponseBody;

/// A window ending between the two bytes of a multi-byte character inside a secret must
/// not store that secret's ASCII prefix: the dangling lead byte is dropped before the
/// lossy conversion, so the straddle trim still sees the real prefix and removes it.
#[test]
fn multi_byte_secret_cut_at_the_window_end_is_not_stored() {
    let ascii_secret = "A".repeat(256);
    let multi_byte_secret = format!("{}é{}", "B".repeat(200), "z".repeat(10));
    let secrets = SecretValues::from_values([ascii_secret.clone(), multi_byte_secret.clone()]);
    let window_len = HISTORY_BODY_LIMIT + secrets.max_len();
    // The window covers the multi-byte secret's first 201 bytes: 200 `B`s and the lead
    // byte of `é`, so the cut lands between that character's two bytes.
    let mut body = ascii_secret.repeat(3).into_bytes();
    body.extend(std::iter::repeat_n(b'x', window_len - 201 - body.len()));
    body.extend_from_slice(multi_byte_secret.as_bytes());

    let prefix = stored_body_prefix(body, &secrets);

    assert!(
        std::str::from_utf8(&prefix).is_ok(),
        "the stored prefix is valid UTF-8"
    );
    assert!(prefix.len() <= HISTORY_BODY_LIMIT, "the prefix is capped");
    assert!(
        !contains(&prefix, b"BBBBBBBB"),
        "no run of the secret's ASCII prefix is stored"
    );
}

/// A body the window holds whole is stored whole: its ending matching a proper prefix of
/// a secret is content, not a straddle, and must not be trimmed away.
#[test]
fn complete_body_ending_like_a_secret_prefix_is_stored_whole() {
    let greeting = br#"{"greeting":"hello"}"#.to_vec();
    let secrets = SecretValues::from_values([
        "o-fake-secret".to_string(),
        "}fake-secret".to_string(),
        "\"fake-secret".to_string(),
    ]);
    assert_eq!(
        stored_body_prefix(greeting.clone(), &secrets),
        greeting,
        "an ending like three secret prefixes keeps every byte"
    );

    let echoed = b"xxxxaaaa".to_vec();
    let secrets = SecretValues::from_values(["aaaab".to_string()]);
    assert_eq!(
        stored_body_prefix(echoed.clone(), &secrets),
        echoed,
        "an ending that is a whole secret prefix keeps every byte"
    );

    let short = b"qqqq".to_vec();
    let secrets = SecretValues::from_values(["q".repeat(5000)]);
    assert_eq!(
        stored_body_prefix(short.clone(), &secrets),
        short,
        "a body shorter than a secret whose prefix it repeats keeps every byte"
    );
}

/// Content beyond the window keeps the straddle trim: an ASCII secret the window cuts
/// part-way through is trimmed whole rather than stored half in the clear.
#[test]
fn body_past_the_window_still_trims_a_straddling_secret() {
    let secret = "token-REDACTED-window-straddle-0042";
    let secrets = SecretValues::from_values([secret.to_string()]);
    let window_len = HISTORY_BODY_LIMIT + secrets.max_len();
    let mut body = vec![b'x'; window_len - 10];
    body.extend_from_slice(secret.as_bytes());
    body.extend_from_slice(b"and the body continues past the window");

    let prefix = stored_body_prefix(body, &secrets);

    assert!(
        !contains(&prefix, &secret.as_bytes()[..8]),
        "the straddling secret is trimmed"
    );
}

/// A streamed body whose file size cannot be read may continue past the window, so a
/// secret its in-memory prefix ends part-way through is still trimmed: an unknown size
/// never makes a cut window look like the whole body.
#[test]
fn file_body_with_unknown_size_still_trims_a_straddling_secret() {
    let secret = "token-REDACTED-unknown-size-0042";
    let secrets = SecretValues::from_values([secret.to_string()]);
    let mut prefix = b"{\"echo\":\"".to_vec();
    prefix.extend_from_slice(&secret.as_bytes()[..20]);
    let missing = std::env::temp_dir().join("pustman-history-window-test-missing-body-file");
    assert!(!missing.exists(), "the fixture path must not exist");

    let stored = stored_prefix_of(
        &ResponseBody::File {
            path: missing,
            prefix,
        },
        &secrets,
    );

    assert!(
        !contains(&stored, &secret.as_bytes()[..8]),
        "the secret's start is not stored: {}",
        String::from_utf8_lossy(&stored)
    );
}

/// The stored body prefix one history row keeps for `body`, through the real window and
/// redaction path.
fn stored_body_prefix(body: Vec<u8>, secrets: &SecretValues) -> Vec<u8> {
    stored_prefix_of(&ResponseBody::Bytes(body), secrets)
}

/// The stored body prefix one history row keeps for any response body shape.
fn stored_prefix_of(body: &ResponseBody, secrets: &SecretValues) -> Vec<u8> {
    let (window, _body_size, may_continue) = body_window_and_size(body, secrets);
    redacted_body_prefix(&window, may_continue, secrets)
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
