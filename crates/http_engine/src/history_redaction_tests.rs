//! The redaction a history row owes its content (REQ-HIST-03, REQ-NFR-04): a secret
//! straddling the body cap, the Basic credential's derived base64 form, and a secret
//! used as a JSON key must never reach a stored row. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method, SecretValues, redact_text};
use base64::Engine;
use gpui::{AppContext, TestAppContext};
use secrets::{MemorySecretStore, SecretKey, SecretStore};
use storage::{HistoryEntry, LocalStore};
use variables::{ScopeKind, ScopeStack};

use super::{HISTORY_BODY_LIMIT, HistoryRecorder};
use crate::{FakeTransport, HttpClient, Response, ResponseBody, SendInput, SpillWriter};

/// The workspace every test records into.
const WORKSPACE: &str = "workspace-under-test";

/// One obviously-fake 256-byte secret: 64 four-character groups whose counters make
/// every proper suffix differ from every prefix, so no tail of a window is a secret
/// prefix by accident and no fragment of it hides in a run of filler bytes.
fn fake_secret(tag: char) -> String {
    (0..64).map(|index| format!("{tag}{index:02}w")).collect()
}

/// A secret whose occurrence straddles the one-megabyte cut is masked whole, not stored
/// half in the clear: the window read goes past the cut, the whole occurrence is masked
/// there, and only then is the prefix cut to the cap.
#[test]
fn history_prefix_masks_a_secret_straddling_the_body_cap() {
    let secret = fake_secret('a');
    let mut body = vec![b'x'; HISTORY_BODY_LIMIT - 100];
    body.extend_from_slice(secret.as_bytes());
    let secrets = SecretValues::from_values([secret.clone()]);

    let prefix = record_body_prefix(ResponseBody::Bytes(body), &secrets);

    assert!(prefix.len() <= HISTORY_BODY_LIMIT, "the prefix is capped");
    assert!(
        !keeps_secret_fragment(&prefix, &secret),
        "no fragment of the straddling secret is stored"
    );
    let mask = redact_text(&secret, &secrets);
    assert!(contains(&prefix, mask.as_bytes()), "the secret is masked");
}

/// The same straddling secret is masked when the body spilled to a temp file and the
/// window is topped up from it.
#[tokio::test]
async fn spilled_history_prefix_masks_a_secret_straddling_the_body_cap() {
    let secret = fake_secret('b');
    let mut body = vec![b'x'; HISTORY_BODY_LIMIT - 100];
    body.extend_from_slice(secret.as_bytes());
    let mut writer = SpillWriter::new(64);
    for chunk in body.chunks(8 * 1024) {
        writer.push(chunk).await.expect("the body spills");
    }
    let spilled = writer.into_body().await.expect("the spilled body");
    assert!(
        matches!(spilled, ResponseBody::Spilled { .. }),
        "the body over the spill limit spills to a temp file"
    );
    let secrets = SecretValues::from_values([secret.clone()]);

    let prefix = record_body_prefix(spilled, &secrets);

    assert!(prefix.len() <= HISTORY_BODY_LIMIT, "the prefix is capped");
    assert!(
        !keeps_secret_fragment(&prefix, &secret),
        "no fragment of the straddling secret is stored"
    );
    let mask = redact_text(&secret, &secrets);
    assert!(contains(&prefix, mask.as_bytes()), "the secret is masked");
}

/// A secret wholly inside the kept prefix is still masked: the straddle fix changes
/// nothing for an occurrence the cut cannot split.
#[test]
fn history_prefix_still_masks_a_secret_wholly_inside_it() {
    let secret = fake_secret('c');
    let mut body = vec![b'y'; 10];
    body.extend_from_slice(secret.as_bytes());
    body.extend(std::iter::repeat_n(b'y', 100));
    let secrets = SecretValues::from_values([secret.clone()]);

    let prefix = record_body_prefix(ResponseBody::Bytes(body), &secrets);

    assert!(
        !keeps_secret_fragment(&prefix, &secret),
        "the secret is masked"
    );
    let mask = redact_text(&secret, &secrets);
    assert!(
        contains(&prefix, mask.as_bytes()),
        "the mask marks where it was"
    );
}

/// Masking can shorten the body — a 256-byte secret becomes a six-glyph mask — and the
/// stored cut is taken after masking: a secret past the cap must not be pulled back
/// inside the kept prefix and kept there half in the clear.
#[test]
fn history_prefix_keeps_no_secret_fragment_earlier_masking_would_pull_in() {
    let first = fake_secret('d');
    let second = fake_secret('e');
    let mut body = first.as_bytes().to_vec();
    body.extend(std::iter::repeat_n(b'x', HISTORY_BODY_LIMIT - 255));
    body.extend_from_slice(second.as_bytes());
    let secrets = SecretValues::from_values([first.clone(), second.clone()]);

    let prefix = record_body_prefix(ResponseBody::Bytes(body), &secrets);

    assert!(prefix.len() <= HISTORY_BODY_LIMIT, "the prefix is capped");
    assert!(
        !keeps_secret_fragment(&prefix, &first),
        "the masked secret leaves no fragment"
    );
    assert!(
        !keeps_secret_fragment(&prefix, &second),
        "the secret past the cap is not pulled into the prefix"
    );
}

/// A registered secret used as a key in `HttpRequest::extra` is masked: JSON keys are
/// request content too.
#[test]
fn history_request_json_masks_a_secret_used_as_an_object_key() {
    let secret = fake_secret('f');
    let store = open_store();
    let recorder = HistoryRecorder::new(store.clone(), WORKSPACE);
    let mut request = plain_request();
    request
        .extra
        .insert(secret.clone(), serde_json::json!("kept"));
    let draft = recorder.draft(&request).expect("the draft builds");
    let secrets = SecretValues::from_values([secret.clone()]);
    let response = Response::new(200, "OK", ResponseBody::Bytes(b"ok".to_vec()));

    draft.record_response(&request, &response, &secrets);

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is recorded");
    let request_json = rows[0].request_json.to_string();
    assert!(
        !request_json.contains(&secret),
        "the secret key is masked in the stored request"
    );
    assert!(
        request_json.contains("kept"),
        "the keyed entry survives the rebuild"
    );
}

/// A server echoing request headers (httpbin `/headers`) puts the Basic credential's
/// base64 form in its body: that derived string is registered among the send's secrets,
/// so the stored prefix holds no encoded credential.
#[gpui::test]
async fn history_masks_the_basic_credential_a_response_body_echoes(cx: &mut TestAppContext) {
    let username = "alice-example";
    let password = "password-REDACTED-example-0042";
    // The wire form, computed here as an independent oracle over the `base64` crate.
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let transport = FakeTransport::with_response(
        cx.executor(),
        200,
        format!("{{\"headers\": {{\"Authorization\": \"Basic {encoded}\"}}}}"),
    );
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });
    let mut request = plain_request();
    request.auth = Auth::Basic {
        username: Some(username.to_string()),
        password: Some(password.to_string()),
        extra: BTreeMap::new(),
    };
    let input = send_input(
        request,
        ScopeStack::new(),
        Arc::new(MemorySecretStore::new()),
    );

    client
        .update(cx, |client, cx| client.send_request(input, cx))
        .await
        .expect("the send succeeds");

    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is recorded");
    let prefix = rows[0]
        .response_body_prefix
        .as_ref()
        .expect("a body prefix is stored");
    assert!(
        contains(prefix, b"headers"),
        "the stored prefix holds the echoed body"
    );
    assert!(
        !contains(prefix, encoded.as_bytes()),
        "the echoed base64 credential is masked"
    );
}

/// Record one response body through a history draft and return the stored body prefix.
fn record_body_prefix(body: ResponseBody, secrets: &SecretValues) -> Vec<u8> {
    let store = open_store();
    let recorder = HistoryRecorder::new(store.clone(), WORKSPACE);
    let request = plain_request();
    let draft = recorder.draft(&request).expect("the draft builds");
    let response = Response::new(200, "OK", body);
    draft.record_response(&request, &response, secrets);
    let rows = stored_rows(&store);
    assert_eq!(rows.len(), 1, "the send is recorded");
    rows[0]
        .response_body_prefix
        .clone()
        .expect("a body prefix is stored")
}

/// Whether any four bytes of `secret` — the shortest fragment redaction exists to
/// destroy — survive in `haystack`.
fn keeps_secret_fragment(haystack: &[u8], secret: &str) -> bool {
    let secret = secret.as_bytes();
    (0..secret.len() - 3).any(|offset| contains(haystack, &secret[offset..offset + 4]))
}

/// One GET to `https://example.test/users` with no params, no body and no own auth.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "history redaction test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// Everything [`HttpClient::send_request`] needs for one send.
fn send_input(
    request: HttpRequest,
    scopes: ScopeStack,
    secrets: Arc<dyn SecretStore>,
) -> SendInput {
    SendInput {
        request,
        ancestors: Vec::new(),
        scopes,
        secrets,
        secret_key: Arc::new(no_secrets),
    }
}

/// No keychain key: these tests' credentials are literals and no secret is fetched.
fn no_secrets(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}

/// The in-memory local store the recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// What the `history` table holds, read back through `storage`.
fn stored_rows(store: &Mutex<LocalStore>) -> Vec<HistoryEntry> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list")
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
