//! Digest auth (RFC 7616): the §3.9.1 vectors, one answered challenge per send, and no
//! credential in any `Debug` or Console line.

use std::sync::Arc;

use api_model::{Body, DigestAlgorithm, Method, redact_headers, redact_text};
use gpui::{AppContext, TestAppContext};

use super::digest_test_support::{
    PASSWORD, RFC_CNONCE, RFC_MD5_CHALLENGE, RFC_PASSWORD, RFC_SHA256_CHALLENGE, RFC_URI,
    RFC_USERNAME, ScriptedTransport, authorization, digest_request, parameter, response,
    send_input,
};
use super::{DigestCredentials, answer_with};
use crate::request::Header;
use crate::{AppliedAuth, HttpClient, SendOutcome, Warning};

/// The RFC example's answer under `algorithm`, both of its challenges offered, with the
/// example's client nonce.
fn rfc_answer(algorithm: DigestAlgorithm) -> String {
    let headers = [
        Header::new("WWW-Authenticate", RFC_SHA256_CHALLENGE),
        Header::new("WWW-Authenticate", RFC_MD5_CHALLENGE),
    ];
    let credentials = DigestCredentials::new(
        RFC_USERNAME.to_string(),
        RFC_PASSWORD.to_string(),
        algorithm,
    );
    answer_with(
        &headers,
        &credentials,
        &Method::Get,
        RFC_URI,
        |client, params| client.respond_with_testing_cnonce(params, RFC_CNONCE),
    )
    .expect("the RFC challenge is answered")
    .authorization
}

/// The parts of the RFC example's `Authorization` every algorithm shares.
fn assert_rfc_fields(authorization: &str) {
    assert_eq!(
        parameter(authorization, "username").as_deref(),
        Some("Mufasa")
    );
    assert_eq!(
        parameter(authorization, "realm").as_deref(),
        Some("http-auth@example.org")
    );
    assert_eq!(parameter(authorization, "uri").as_deref(), Some(RFC_URI));
    assert_eq!(parameter(authorization, "qop").as_deref(), Some("auth"));
    assert_eq!(parameter(authorization, "nc").as_deref(), Some("00000001"));
    assert_eq!(
        parameter(authorization, "cnonce").as_deref(),
        Some(RFC_CNONCE)
    );
    assert_eq!(
        parameter(authorization, "opaque").as_deref(),
        Some("FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS")
    );
}

#[test]
fn rfc7616_md5_vector_matches() {
    let authorization = rfc_answer(DigestAlgorithm::Md5);

    assert_rfc_fields(&authorization);
    assert_eq!(
        parameter(&authorization, "algorithm").as_deref(),
        Some("MD5")
    );
    assert_eq!(
        parameter(&authorization, "response").as_deref(),
        Some("8ca523f5e9506fed4657c9700eebdbec")
    );
}

#[test]
fn rfc7616_sha256_vector_matches() {
    let authorization = rfc_answer(DigestAlgorithm::Sha256);

    assert_rfc_fields(&authorization);
    assert_eq!(
        parameter(&authorization, "algorithm").as_deref(),
        Some("SHA-256")
    );
    assert_eq!(
        parameter(&authorization, "response").as_deref(),
        Some("753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1")
    );
}

/// No credential goes out before the challenge, the challenge is answered once, and the
/// server's second 401 is the send's response — the third scripted answer is never
/// asked for.
#[gpui::test]
async fn challenge_answered_once_then_401_returned(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[RFC_SHA256_CHALLENGE], "first"),
        response(401, &[RFC_SHA256_CHALLENGE], "second"),
        response(200, &[], "never asked for"),
    ]);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let request = digest_request(Method::Get, DigestAlgorithm::Sha256, Body::None);

    let outcome = client
        .update(cx, |client, cx| {
            client.send_request(send_input(request), cx)
        })
        .await
        .expect("the send completes");

    let requests = transport.requests();
    assert_eq!(requests.len(), 2, "one request, one answer, nothing more");
    assert_eq!(
        authorization(&requests[0]),
        None,
        "no credential before the challenge"
    );
    let answer = authorization(&requests[1]).expect("the answer carries Authorization");
    assert!(answer.starts_with("Digest "), "a Digest answer");
    assert_eq!(
        parameter(answer, "uri").as_deref(),
        Some("/dir/index.html?page=1")
    );
    assert_eq!(parameter(answer, "algorithm").as_deref(), Some("SHA-256"));
    assert_eq!(parameter(answer, "qop").as_deref(), Some("auth"));
    assert_eq!(parameter(answer, "nc").as_deref(), Some("00000001"));
    assert_eq!(
        outcome.response.status, 401,
        "the second 401 reaches the user"
    );
    assert_eq!(outcome.response.body.as_bytes(), b"second");
    assert_eq!(outcome.auth, AppliedAuth::Applied);
    assert!(
        !outcome
            .warnings
            .iter()
            .any(|warning| matches!(warning, Warning::AuthUnsupported { .. })),
        "an answered challenge is not reported as unsupported"
    );
}

/// The password and the computed response never show in `Debug` output, and the
/// Console's lines — headers through `redact_headers`, text through `redact_text` with
/// the send's secrets — mask the whole `Authorization` value and its hash.
#[gpui::test]
async fn digest_header_never_logged(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[RFC_MD5_CHALLENGE], ""),
        response(200, &[], "ok"),
    ]);
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let request = digest_request(Method::Get, DigestAlgorithm::Md5, Body::None);

    let outcome: SendOutcome = client
        .update(cx, |client, cx| {
            client.send_request(send_input(request), cx)
        })
        .await
        .expect("the send completes");

    let answer = authorization(&outcome.request)
        .expect("the request as sent carries the answer")
        .to_string();
    let hash = parameter(&answer, "response").expect("the answer has a response");
    let forbidden = [PASSWORD, answer.as_str(), hash.as_str()];

    let debug = [
        format!("{outcome:?}"),
        format!("{:?}", transport.requests()),
        format!(
            "{:?}",
            DigestCredentials::new("u".into(), PASSWORD.into(), DigestAlgorithm::Md5)
        ),
    ];
    let headers: Vec<(String, String)> = outcome
        .request
        .headers
        .iter()
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect();
    let console = [
        format!("{:?}", redact_headers(&headers, &outcome.secrets_used)),
        redact_text(&format!("Authorization: {answer}"), &outcome.secrets_used),
        redact_text(&format!("echo {hash}"), &outcome.secrets_used),
    ];
    for line in debug.iter().chain(console.iter()) {
        for secret in forbidden {
            assert!(!line.contains(secret), "a Digest credential leaked: {line}");
        }
    }
    assert_eq!(
        outcome.secrets_used.refused_count(),
        0,
        "every value is maskable"
    );
}
