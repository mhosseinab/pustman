//! What a send owes the thread driving the UI and its log lines: no client build on
//! the calling thread, and no resolved URL in an error's `Debug` text (PRD §7,
//! AGENTS.md › Rules › GPUI).

use std::sync::mpsc;
use std::time::Duration;

use futures::future::BoxFuture;
use url::Url;

use super::{
    ReqwestTransport, headers_for_redirect, interrupted_detail, lock, map_error, tls_error_text,
    tls_reason,
};
use crate::{
    Header, RequestBody, ResolvedRequest, Response, SendSettings, TestServer, Transport,
    TransportError,
};

/// `TransportError::Io`'s `Debug` prints `detail` as it finds it, and `.log_err()` goes
/// through `Debug` — so a `reqwest` error may never be stringified into `detail` with
/// its URL attached: `Display` appends the resolved URL, query values and all (PRD §7).
/// A request `reqwest` refuses to build carries its URL into exactly that fallback.
#[tokio::test]
async fn an_io_error_never_carries_a_secret_query_value_in_its_debug_output() {
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "ftp://example.test/collect?api_key=token-REDACTED-example",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let error = transport
        .send(request)
        .await
        .expect_err("a scheme reqwest refuses is a failed send");

    assert!(
        matches!(error, TransportError::Io { .. }),
        "the §8.6 fallback lands in `Io`: {error:?}"
    );
    assert!(
        !format!("{error:?}").contains("token-REDACTED-example"),
        "the secret query value reached the Debug output: {error:?}"
    );
}

/// Building a `reqwest` client loads the system root certificate store — file IO that
/// must never run on the thread driving the UI (AGENTS.md › Rules › GPUI), and that is
/// where `HttpClient::send` calls `Transport::send`. Neither the transport nor the
/// synchronous part of `send` builds one: the client appears only when the spawned
/// tokio task runs.
#[test]
fn the_synchronous_part_of_send_builds_no_client_on_the_calling_thread() {
    // A current-thread runtime runs its tasks only on the thread that drives it — the
    // driver below — so a client that appears in the cache after the driver has run the
    // send was built there, never on this thread.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    let handle = runtime.handle().clone();
    let (future_sender, future_receiver) =
        mpsc::channel::<BoxFuture<'static, Result<Response, TransportError>>>();
    let (outcome_sender, outcome_receiver) = mpsc::channel();
    let driver = std::thread::spawn(move || {
        let future = future_receiver.recv().expect("the send future");
        let outcome = runtime.block_on(future);
        outcome_sender.send(outcome).expect("the outcome channel");
    });

    let transport = ReqwestTransport::new(handle);
    assert!(
        lock(&transport.clients).is_empty(),
        "constructing the transport built a client on the calling thread"
    );

    let request = ResolvedRequest::new(
        api_model::Method::Get,
        &format!("http://127.0.0.1:{}/", port_with_no_listener()),
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");
    let future = transport.send(request);
    assert!(
        lock(&transport.clients).is_empty(),
        "the synchronous part of `send` built a client on the calling thread"
    );

    future_sender.send(future).expect("the future channel");
    let outcome = outcome_receiver.recv().expect("the send outcome");
    assert!(
        matches!(outcome, Err(TransportError::Connect { .. })),
        "the send ran to a refused port and mapped it to Connect: {outcome:?}"
    );
    assert_eq!(
        lock(&transport.clients).len(),
        1,
        "the spawned task built the client where it ran"
    );
    driver.join().expect("the driver thread");
}

/// A failed send whose resolved URL contains one of the TLS marker words is a plain
/// `Connect` failure (§8.6) — and no `Debug` output names that URL. Before the fix the
/// marker scan ran on `reqwest`'s URL-carrying `Display`, so this very request was
/// misfiled as `Tls` and printed the resolved URL — query values and all — into
/// `Tls.reason` (PRD §7).
#[tokio::test]
async fn a_connect_refused_at_a_marker_word_url_is_connect_and_keeps_the_url_out_of_debug() {
    let port = port_with_no_listener();
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        &format!("http://127.0.0.1:{port}/handshake?api_key=token-REDACTED-example"),
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let error = transport
        .send(request)
        .await
        .expect_err("a refused port is a failed send");

    assert!(
        matches!(error, TransportError::Connect { .. }),
        "a refused port is a Connect failure, not a TLS one: {error:?}"
    );
    let debug = format!("{error:?}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "the secret query value reached the Debug output: {debug}"
    );
    assert!(
        !debug.contains("handshake"),
        "the resolved URL reached the Debug output: {debug}"
    );
}

/// A name that does not resolve is a `Dns` failure whatever its URL says (§8.6) — a
/// DNS failure is never a handshake failure — and the resolved URL reaches no `Debug`
/// output (PRD §7).
#[tokio::test]
async fn a_dns_failure_at_a_marker_word_url_is_dns_and_keeps_the_url_out_of_debug() {
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "http://handshake.invalid/handshake?api_key=token-REDACTED-example",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let error = transport
        .send(request)
        .await
        .expect_err(".invalid never resolves");

    assert!(
        matches!(error, TransportError::Dns { .. }),
        "a name that does not resolve is a Dns failure, not a TLS one: {error:?}"
    );
    let debug = format!("{error:?}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "the secret query value reached the Debug output: {debug}"
    );
}

/// `tls_error_text` never marker-scans a `reqwest` error: its `Display` appends the
/// resolved URL (PRD §7), so a marker word in the URL is not evidence of a handshake
/// failure — and stringifying that text would put the URL into `Tls.reason`.
#[tokio::test]
async fn a_reqwest_error_is_never_scanned_for_tls_markers() {
    let port = port_with_no_listener();
    let client = reqwest::Client::builder()
        .build()
        .expect("a reqwest client");
    let error = client
        .get(format!(
            "http://127.0.0.1:{port}/handshake?api_key=token-REDACTED-example"
        ))
        .send()
        .await
        .expect_err("a refused port is a failed send");
    assert!(
        error.to_string().contains("handshake"),
        "the test premise — this error's Display carries the resolved URL: {error}"
    );

    assert!(
        tls_error_text(&error).is_none(),
        "a reqwest error's URL-carrying text was read as TLS evidence"
    );
}

/// The marker words still work on the errors that are not `reqwest`'s: the wording of a
/// real handshake failure still reaches `Tls.reason`.
#[test]
fn the_tls_markers_still_catch_the_wording_of_a_real_handshake_failure() {
    let error = HandshakeWording("invalid peer certificate: UnknownIssuer");

    assert_eq!(
        tls_error_text(&error).as_deref(),
        Some("invalid peer certificate: UnknownIssuer")
    );
}

/// A certificate issued for another name is the one TLS failure whose rustls wording
/// repeats the REQUESTED server name (`NotValidForNameContext` names it), and the
/// requested name is resolved text — a secret resolves into a host just as it does into
/// a path (PRD §7, `error.rs`). The reason keeps the fact of the mismatch and drops
/// every name in it (H1).
#[tokio::test]
async fn a_hostname_mismatch_never_puts_the_requested_name_into_the_tls_reason() {
    let server = TestServer::start_tls().await.expect("a TLS test server");
    let certificate =
        reqwest::Certificate::from_der(server.certificate_der().expect("the served certificate"))
            .expect("the certificate for reqwest");
    let port = server.url().port().expect("a bound port");
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    // The certificate is trusted but names `localhost` and `127.0.0.1` only, so this
    // handshake fails on the NAME — with the secret-carrying requested name (PRD §7).
    let client = reqwest::Client::builder()
        .add_root_certificate(certificate)
        .resolve("token-REDACTED-example.example.test", address)
        .build()
        .expect("a client that trusts the test certificate");
    let url = Url::parse(&format!(
        "https://token-REDACTED-example.example.test:{port}/"
    ))
    .expect("a valid URL");
    let error = client
        .get(url.clone())
        .send()
        .await
        .expect_err("the certificate is issued for other names");

    let error = map_error(error, Duration::from_secs(30), &url);

    match &error {
        TransportError::Tls { reason } => {
            assert!(
                reason.contains("certificate not valid for name"),
                "the mismatch is no longer named: {reason}"
            );
            // The URL lowercases its host before the name check, so both spellings are
            // watched: a resolved secret goes on the wire as the host carries it.
            for leaked in ["token-REDACTED-example", "token-redacted-example"] {
                assert!(
                    !reason.contains(leaked),
                    "`{leaked}` reached the reason: {reason}"
                );
            }
        }
        other => panic!("a name mismatch is a TLS failure, got {other:?}"),
    }
    let debug = format!("{error:?}");
    for leaked in ["token-REDACTED-example", "token-redacted-example"] {
        assert!(
            !debug.contains(leaked),
            "`{leaked}` reached the Debug output: {debug}"
        );
    }
}

/// The same mismatch arriving through an `io::Error` wrapper — the shape `tokio-rustls`
/// hands up — is caught by the marker scan, and its wording is sanitized the same way
/// before it can reach `Tls.reason` (H1).
#[test]
fn the_hostname_mismatch_marker_wording_is_sanitized_before_it_reaches_the_reason() {
    let error = std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        HandshakeWording(
            "invalid peer certificate: certificate not valid for name \
             \"token-REDACTED-example.example.test\"; certificate is only valid for \
             \"localhost\"",
        ),
    );

    let reason = tls_error_text(&error).expect("the marker wording is TLS evidence");

    assert_eq!(
        reason,
        "invalid peer certificate: certificate not valid for name"
    );
}

/// rustls-platform-verifier wraps the OS trust verdict as `CertificateError::Other(..)`
/// on macOS and Windows, and that payload is opaque text that can embed the REQUESTED
/// host (PRD §7) while `Tls.reason` prints unredacted in `Debug` (`error.rs`): a
/// platform `Other` reason is replaced wholesale by a static one (H1) and no marker of
/// the payload survives it. Both shapes the verdict arrives in are pinned — the `rustls`
/// error itself and the `io::Error` wrapper `tokio-rustls` hands up — and the wording as
/// [`tls_reason`] receives it.
#[test]
fn a_platform_other_error_never_puts_the_requested_name_into_the_tls_reason() {
    let error = rustls::Error::InvalidCertificate(rustls::CertificateError::Other(
        rustls::OtherError(std::sync::Arc::new(HandshakeWording(
            "the platform verdict named token-REDACTED-example.example.test",
        ))),
    ));
    let rendered = error.to_string();
    assert!(
        rendered.contains("token-REDACTED-example.example.test"),
        "the test premise — the rendered error carries the marker: {rendered}"
    );

    for reason in [
        tls_error_text(&error).expect("a rustls error is TLS evidence"),
        tls_error_text(&std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.clone(),
        ))
        .expect("the tokio-rustls wrapper reads through to the rustls error"),
        tls_reason(&rendered),
    ] {
        assert!(
            !reason.contains("token-REDACTED-example.example.test"),
            "the platform verdict reached the reason: {reason}"
        );
    }
}

/// A scheme downgrade is a cross-origin hop too: the same host and port over cleartext
/// is exactly where a cookie must not go (H2). `keep_authorization` still reaches
/// `Authorization` — the one override, for the one header — and saves nothing else.
#[test]
fn a_scheme_downgrade_hop_drops_the_cookies_and_proxy_credentials() {
    let previous = Url::parse("https://api.example.test:8443/start").expect("a valid URL");
    let next = Url::parse("http://api.example.test:8443/next").expect("a valid URL");

    let kept = headers_for_redirect(
        vec![
            Header::new("Cookie", "session=cookie-secret"),
            Header::new("Cookie2", "session2=cookie2-secret"),
            Header::new("Proxy-Authorization", "Basic proxy-secret"),
            Header::new("Authorization", "Bearer authorization-secret"),
            Header::new("X-Trace", "trace-kept"),
        ],
        &previous,
        &next,
        true,
    );

    let names: Vec<&str> = kept.iter().map(|header| header.name.as_str()).collect();
    assert_eq!(names, ["Authorization", "X-Trace"]);
}

/// The wire name is case-insensitive (RFC 9110 §5.1), and the cross-origin drop is
/// too: `COOKIE`, `cOoKiE2` and `Proxy-AUTHORIZATION` are the same headers to the wire
/// and stop at the hop exactly like their canonical spellings.
#[test]
fn the_cross_origin_cookie_strip_ignores_the_header_name_casing() {
    let previous = Url::parse("https://api.example.test/start").expect("a valid URL");
    let next = Url::parse("https://other.example.test/next").expect("a valid URL");

    let kept = headers_for_redirect(
        vec![
            Header::new("COOKIE", "session=cookie-secret"),
            Header::new("cOoKiE2", "session2=cookie2-secret"),
            Header::new("Proxy-AUTHORIZATION", "Basic proxy-secret"),
            Header::new("X-Trace", "trace-kept"),
        ],
        &previous,
        &next,
        true,
    );

    let names: Vec<&str> = kept.iter().map(|header| header.name.as_str()).collect();
    assert_eq!(names, ["X-Trace"]);
}

/// A header name can carry a resolved secret — variables resolve into header names too
/// (PRD §7) — and `Io`'s `Debug` prints `detail` as it finds it (`error.rs`): a name the
/// wire format refuses is named by its position and never echoed.
#[tokio::test]
async fn an_invalid_header_name_error_names_the_position_and_never_the_name() {
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "http://example.test/",
        vec![Header::new("X token-REDACTED-example", "value")],
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let error = transport
        .send(request)
        .await
        .expect_err("an invalid header name is a failed send");

    let debug = format!("{error:?}");
    assert!(
        debug.contains("header #1"),
        "the message no longer says which header: {debug}"
    );
    assert!(
        !debug.contains("token-REDACTED-example"),
        "the resolved name reached the Debug output: {debug}"
    );
}

/// The same rule for a value the wire format refuses: the message names the header's
/// position and echoes neither the name nor the value — a name carries a resolved
/// secret just as a value can (PRD §7).
#[tokio::test]
async fn an_invalid_header_value_error_names_the_position_and_never_the_name() {
    let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "http://example.test/",
        vec![Header::new(
            "X-token-REDACTED-example",
            "value\nwith-newline",
        )],
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    let error = transport
        .send(request)
        .await
        .expect_err("an invalid header value is a failed send");

    let debug = format!("{error:?}");
    assert!(
        debug.contains("header #1"),
        "the message no longer says which header: {debug}"
    );
    assert!(
        !debug.contains("token-REDACTED-example"),
        "the resolved name reached the Debug output: {debug}"
    );
}

/// A header NAME can carry a resolved secret — variables resolve into header names too
/// (PRD §7) — and `Header`'s `Debug` reaches logs through `ResolvedRequest`'s. A name
/// that is not a well-known header name is redacted exactly like a value (`request.rs`).
#[test]
fn test_debug_of_a_custom_header_name_does_not_reveal_it() {
    let header = Header::new("x-sekret-9f3a", "v");
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "http://example.test/",
        vec![Header::new("x-sekret-9f3a", "v")],
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    for debug in [format!("{header:?}"), format!("{request:?}")] {
        assert!(
            !debug.contains("x-sekret-9f3a"),
            "the secret name reached the Debug output: {debug}"
        );
        // The value's `Debug` text is `"v"` — quoted — so the quoted form is what a
        // leak would print.
        assert!(
            !debug.contains("\"v\""),
            "the value reached the Debug output: {debug}"
        );
        assert!(
            debug.contains("<redacted>"),
            "nothing is marked redacted: {debug}"
        );
    }
}

/// A well-known header name is a standard wire name, not user text, so it stays
/// visible whatever its case (`request.rs`) — `request_tests.rs` pins
/// `Authorization` visible and is read-only on the integration branch.
#[test]
fn test_debug_of_a_well_known_header_name_stays_visible_in_any_case() {
    let lowercase = Header::new("content-type", "application/json");
    let uppercase = Header::new("AUTHORIZATION", "Bearer token-REDACTED-example");
    let request = ResolvedRequest::new(
        api_model::Method::Get,
        "http://example.test/",
        vec![lowercase.clone(), uppercase.clone()],
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("a request with a resolvable URL");

    assert!(
        format!("{lowercase:?}").contains("content-type"),
        "name hidden: {lowercase:?}"
    );
    assert!(
        format!("{uppercase:?}").contains("AUTHORIZATION"),
        "name hidden: {uppercase:?}"
    );
    let debug = format!("{request:?}");
    assert!(debug.contains("content-type"), "name hidden: {debug}");
    assert!(debug.contains("AUTHORIZATION"), "name hidden: {debug}");
    assert!(
        !debug.contains("token-REDACTED-example"),
        "the value leaked: {debug}"
    );
}

/// A task that dies with a panic: the `Io` detail says how the task ended and nothing
/// the panic payload carried — tokio's `JoinError` `Display` embeds the payload text
/// and `Io`'s `Debug` prints `detail` unredacted (PRD §7, `error.rs`).
#[tokio::test]
async fn an_interrupted_task_detail_never_carries_the_panic_payload() {
    let task = tokio::spawn(async {
        panic!("panicking with token-REDACTED-example on the stack");
    });
    let join_error = task.await.expect_err("a panicked task ends in a JoinError");

    let detail = interrupted_detail("The send was interrupted", &join_error);

    assert!(detail.contains("The send was interrupted"), "{detail}");
    assert!(detail.contains("panicked"), "{detail}");
    assert!(
        !detail.contains("token-REDACTED-example"),
        "the panic payload reached the detail: {detail}"
    );
}

/// The wording [`tls_error_text`] looks for, as a plain error — what a real handshake
/// failure looks like once it is out of `reqwest`'s wrapper.
#[derive(Debug)]
struct HandshakeWording(&'static str);

impl std::fmt::Display for HandshakeWording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for HandshakeWording {}

/// A port nothing listens on: bind one to learn its number, then let it go again.
fn port_with_no_listener() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
    listener.local_addr().expect("the bound address").port()
}
