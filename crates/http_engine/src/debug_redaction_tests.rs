//! The `Debug` redaction rules (PRD §7): a resolved secret never reaches a field a
//! `Debug` prints — the resolved URL and its host and path, and a body file's path all
//! carry resolved values. What `Display` and `user_message()` show the user is real
//! text; only `Debug` — what error logging formats — is redacted.

use std::path::PathBuf;

use url::Url;

use crate::error::TransportError;
use crate::redacted_url;
use crate::request::{RequestBody, ResolvedRequest, SendSettings};
use crate::response::{RedirectHop, ResponseBody};

/// A webhook-style URL, the standard shape for a secret carried in a path.
fn webhook_url() -> Url {
    Url::parse(
        "https://user:pass@hooks.example.test/services/T000/B000/token-REDACTED-example\
         ?api_key=query-secret#fragment-secret",
    )
    .expect("a valid URL")
}

/// Every part of a URL that can carry a resolved secret is gone from `redacted_url`:
/// userinfo, host, port, path, query and fragment. Only the scheme — whether the send
/// used TLS at all — survives, the rule `TransportError`'s `Debug` follows (`error.rs`).
#[test]
fn redacted_url_keeps_nothing_but_the_scheme() {
    let shown = redacted_url(&webhook_url());

    for secret in [
        "user",
        "pass",
        "hooks.example.test",
        "T000",
        "B000",
        "token-REDACTED-example",
        "query-secret",
        "fragment-secret",
    ] {
        assert!(!shown.contains(secret), "`{secret}` leaked: {shown}");
    }
    assert_eq!(shown, "https://<redacted>");
}

/// The redirect surfaces — one hop's `Debug`, hence `TooManyRedirects.chain` and
/// `Response::Debug`'s `redirects` — never name a hop's URL or its `Location`.
#[test]
fn a_redirect_chain_debug_keeps_no_part_of_a_hop_url() {
    let hop = RedirectHop {
        method: api_model::Method::Get,
        url: webhook_url(),
        status: 302,
        headers: Vec::new(),
        location: Some(
            Url::parse("https://hooks.example.test/services/T000/B000/token-REDACTED-example/next")
                .expect("a valid URL"),
        ),
        timing: None,
    };

    let debug = format!(
        "{:?}",
        TransportError::TooManyRedirects { chain: vec![hop] }
    );

    assert!(
        !debug.contains("token-REDACTED-example"),
        "a path secret reached the Debug output: {debug}"
    );
    assert!(
        !debug.contains("hooks.example.test"),
        "a hop host reached the Debug output: {debug}"
    );
}

/// A body file's path never reaches `Debug`, in either body type: `RequestBody::File`
/// and `ResponseBody::File` redact it exactly as `TransportError::FileNotFound` does
/// (`error.rs`) — a body path can carry a resolved secret (REQ-VAR-01).
#[test]
fn a_body_file_path_never_reaches_debug_output() {
    let request_body = RequestBody::File(PathBuf::from("/home/token-REDACTED-example/avatar.png"));
    let request_debug = format!("{request_body:?}");
    assert!(
        !request_debug.contains("token-REDACTED-example"),
        "a path secret reached the Debug output: {request_debug}"
    );
    assert!(
        !request_debug.contains("home"),
        "the path reached the Debug output: {request_debug}"
    );
    assert!(
        request_debug.contains("<redacted>"),
        "no redaction marker: {request_debug}"
    );

    let response_body = ResponseBody::File {
        path: PathBuf::from("/home/token-REDACTED-example/download.bin"),
        prefix: Vec::new(),
    };
    let response_debug = format!("{response_body:?}");
    assert!(
        !response_debug.contains("token-REDACTED-example"),
        "a path secret reached the Debug output: {response_debug}"
    );
    assert!(
        !response_debug.contains("home"),
        "the path reached the Debug output: {response_debug}"
    );
    assert!(
        response_debug.contains("<redacted>"),
        "no redaction marker: {response_debug}"
    );
}

/// The one place a body file's NAME still shows: `ResolvedRequest`'s `Debug`, where
/// `request_tests.rs` (`debug_output_describes_a_file_body_by_path`) pins it visible in
/// a test file that is read-only on the integration branch. The directory — where a
/// resolved variable puts its secret — never appears there either.
#[test]
fn a_request_debug_names_the_body_file_but_never_its_directory() {
    let request = ResolvedRequest::new(
        api_model::Method::Put,
        "https://example.test/avatar",
        Vec::new(),
        RequestBody::File(PathBuf::from(
            "/home/token-REDACTED-example/fixtures/avatar.png",
        )),
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let debug = format!("{request:?}");

    assert!(
        debug.contains("avatar.png"),
        "the pinned file name went missing: {debug}"
    );
    assert!(
        !debug.contains("token-REDACTED-example"),
        "a path secret reached the Debug output: {debug}"
    );
    assert!(
        !debug.contains("fixtures"),
        "the directory reached the Debug output: {debug}"
    );
}
