//! `HttpClient`'s answer to a Digest 401 (S55a): what it leaves in History and the
//! Console, and every case where the 401 goes back to the user unanswered, with the
//! reason. Fixture credentials are obviously fake (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Body, DigestAlgorithm, Method, RawLanguage, redact_headers, redact_text};
use gpui::{AppContext, TestAppContext};
use storage::{HistoryEntry, LocalStore};
use url::Url;

use crate::digest::digest_test_support::{
    PASSWORD, RFC_MD5_CHALLENGE, RFC_SHA256_CHALLENGE, ScriptedTransport, authorization,
    digest_request, parameter, response, send_input,
};
use crate::{
    AppliedAuth, HistoryRecorder, HttpClient, RedirectHop, RequestBody, SendOutcome, Warning,
};

const WORKSPACE: &str = "workspace-under-test";

/// The RFC's SHA-256 challenge offering only `auth-int`.
const AUTH_INT_ONLY: &str = r#"Digest realm="api@example.test", qop="auth-int", algorithm=SHA-256, nonce="nonce-example-0001""#;

/// An `MD5-sess` challenge.
const MD5_SESS: &str = r#"Digest realm="api@example.test", qop="auth", algorithm=MD5-sess, nonce="nonce-example-0002""#;

/// Send `request` through a client over `transport`, recording history into `store`
/// when given.
async fn send(
    cx: &mut TestAppContext,
    transport: &ScriptedTransport,
    request: api_model::HttpRequest,
    store: Option<Arc<Mutex<LocalStore>>>,
) -> SendOutcome {
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    if let Some(store) = store {
        client.update(cx, |client, _| {
            client.set_history(HistoryRecorder::new(store, WORKSPACE));
        });
    }
    client
        .update(cx, |client, cx| {
            client.send_request(send_input(request), cx)
        })
        .await
        .expect("the send completes")
}

/// The warning text of an unanswered challenge, if there is one.
fn unsupported(outcome: &SendOutcome) -> Option<String> {
    outcome.warnings.iter().find_map(|warning| match warning {
        Warning::AuthUnsupported { .. } => Some(warning.user_message()),
        _ => None,
    })
}

/// A 401 that went back unanswered: one request, no credential sent, the 401 shown, and
/// the reason told through the auth warning.
fn assert_unanswered(transport: &ScriptedTransport, outcome: &SendOutcome, reason: &str) {
    let requests = transport.requests();
    assert_eq!(requests.len(), 1, "the challenge is not answered");
    assert_eq!(authorization(&requests[0]), None, "no credential was sent");
    assert_eq!(outcome.response.status, 401, "the 401 reaches the user");
    assert!(
        matches!(outcome.auth, AppliedAuth::Unsupported { .. }),
        "{:?}",
        outcome.auth
    );
    let message = unsupported(outcome).expect("the user is told why");
    assert!(message.contains(reason), "{message}");
}

#[gpui::test]
async fn digest_retry_leaves_no_password_or_response_hash_in_history_or_console(
    cx: &mut TestAppContext,
) {
    let store = open_store();
    // The server echoes the answer back (httpbin `/headers`), in its body and a header.
    let challenge = ScriptedTransport::new([response(401, &[RFC_SHA256_CHALLENGE], "")]);
    let request = digest_request(Method::Get, DigestAlgorithm::Sha256, Body::None);

    let outcome = send_echoed(cx, challenge, request, store.clone()).await;

    let answer = authorization(&outcome.request)
        .expect("the request as sent carries the answer")
        .to_string();
    let hash = parameter(&answer, "response").expect("the answer has a response");
    let forbidden = [PASSWORD, answer.as_str(), hash.as_str()];

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is one history row, not two");
    let row = &rows[0];
    let stored = [
        row.target.clone(),
        row.request_json.to_string(),
        format!("{:?}", row.response_summary_json),
        String::from_utf8_lossy(row.response_body_prefix.as_deref().unwrap_or(&[])).into_owned(),
    ];
    assert!(
        stored.iter().any(|text| text.contains("echo")),
        "the echoed body is stored: {stored:?}"
    );

    let headers: Vec<(String, String)> = outcome
        .request
        .headers
        .iter()
        .chain(outcome.response.headers.iter())
        .map(|header| (header.name.clone(), header.value.clone()))
        .collect();
    let console = [
        format!("{:?}", redact_headers(&headers, &outcome.secrets_used)),
        redact_text(
            &String::from_utf8_lossy(outcome.response.body.as_bytes()),
            &outcome.secrets_used,
        ),
    ];
    for text in stored.iter().chain(console.iter()) {
        for secret in forbidden {
            assert!(!text.contains(secret), "a Digest credential leaked: {text}");
        }
    }
}

#[gpui::test]
async fn a_sha256_digest_is_never_answered_with_an_md5_challenge(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[RFC_MD5_CHALLENGE], ""),
        response(200, &[], "never asked for"),
    ]);
    let request = digest_request(Method::Get, DigestAlgorithm::Sha256, Body::None);

    let outcome = send(cx, &transport, request, None).await;

    assert_unanswered(&transport, &outcome, "SHA-256");
}

#[gpui::test]
async fn an_auth_int_only_challenge_returns_the_401_with_the_reason(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[AUTH_INT_ONLY], ""),
        response(200, &[], "never asked for"),
    ]);
    let request = digest_request(Method::Get, DigestAlgorithm::Sha256, Body::None);

    let outcome = send(cx, &transport, request, None).await;

    assert_unanswered(&transport, &outcome, "auth-int");
}

#[gpui::test]
async fn a_401_after_a_redirect_is_not_answered(cx: &mut TestAppContext) {
    let mut redirected = response(401, &[RFC_SHA256_CHALLENGE], "");
    let other = Url::parse("https://other.example.test/login").expect("a valid URL");
    redirected.redirects = vec![RedirectHop {
        method: Method::Get,
        url: Url::parse("https://example.test/dir/index.html?page=1").expect("a valid URL"),
        status: 302,
        headers: Vec::new(),
        location: Some(other),
        timing: None,
    }];
    let transport = ScriptedTransport::new([redirected, response(200, &[], "never asked for")]);
    let request = digest_request(Method::Get, DigestAlgorithm::Sha256, Body::None);

    let outcome = send(cx, &transport, request, None).await;

    assert_unanswered(&transport, &outcome, "redirect");
}

#[gpui::test]
async fn a_file_body_is_not_replayed_for_a_digest_answer(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[RFC_SHA256_CHALLENGE], ""),
        response(200, &[], "never asked for"),
    ]);
    let body = Body::Binary {
        file: "upload-example.bin".to_string(),
        extra: BTreeMap::new(),
    };
    let request = digest_request(Method::Post, DigestAlgorithm::Sha256, body);

    let outcome = send(cx, &transport, request, None).await;

    assert_unanswered(&transport, &outcome, "file or form-data body");
}

#[gpui::test]
async fn an_md5_sess_challenge_is_answered_with_md5_sess(cx: &mut TestAppContext) {
    let transport =
        ScriptedTransport::new([response(401, &[MD5_SESS], ""), response(200, &[], "ok")]);
    let request = digest_request(Method::Get, DigestAlgorithm::Md5Sess, Body::None);

    let outcome = send(cx, &transport, request, None).await;

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    let answer = authorization(&requests[1]).expect("the answer carries Authorization");
    assert_eq!(parameter(answer, "algorithm").as_deref(), Some("MD5-sess"));
    assert_eq!(outcome.response.status, 200);
}

#[gpui::test]
async fn the_digest_answer_resends_the_same_body_bytes(cx: &mut TestAppContext) {
    let transport = ScriptedTransport::new([
        response(401, &[RFC_SHA256_CHALLENGE], ""),
        response(201, &[], "created"),
    ]);
    let body = Body::Raw {
        language: RawLanguage::Json,
        text: r#"{"name":"pustman"}"#.to_string(),
        extra: BTreeMap::new(),
    };
    let request = digest_request(Method::Post, DigestAlgorithm::Sha256, body);

    let outcome = send(cx, &transport, request, None).await;

    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body, "the same bytes again");
    assert_eq!(
        requests[1].body,
        RequestBody::Bytes(br#"{"name":"pustman"}"#.to_vec())
    );
    assert_eq!(requests[0].method, requests[1].method);
    assert_eq!(outcome.response.status, 201);
}

/// Send `request`, recorded into `store`, to a server that answers an unauthenticated
/// request through `challenge` and an answered one by echoing its `Authorization` value
/// back, in the body and in a response header — a server reflecting request headers.
async fn send_echoed(
    cx: &mut TestAppContext,
    challenge: ScriptedTransport,
    request: api_model::HttpRequest,
    store: Arc<Mutex<LocalStore>>,
) -> SendOutcome {
    let client = cx.new(|_| HttpClient::new(Arc::new(EchoTransport { challenge })));
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store, WORKSPACE));
    });
    client
        .update(cx, |client, cx| {
            client.send_request(send_input(request), cx)
        })
        .await
        .expect("the send completes")
}

struct EchoTransport {
    challenge: ScriptedTransport,
}

impl crate::Transport for EchoTransport {
    fn send(
        &self,
        request: crate::ResolvedRequest,
    ) -> futures::future::BoxFuture<'static, Result<crate::Response, crate::TransportError>> {
        match authorization(&request).map(str::to_string) {
            None => self.challenge.send(request),
            Some(answer) => {
                let mut echoed = response(200, &[], &format!("echo {answer}"));
                echoed
                    .headers
                    .push(crate::Header::new("X-Echo-Authorization", answer));
                Box::pin(async move { Ok(echoed) })
            }
        }
    }
}

/// The in-memory local store the recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// What the `history` table holds.
fn stored_rows(store: &Mutex<LocalStore>) -> Vec<HistoryEntry> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list")
}
