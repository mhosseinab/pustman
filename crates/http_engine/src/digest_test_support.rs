//! Shared fixtures of the Digest tests (`digest_tests.rs`, `digest_retry_tests.rs`): a
//! transport scripted with whole responses — `WWW-Authenticate` fields included, which
//! `FakeTransport` cannot script — and the RFC 7616 §3.9.1 example. Fixture credentials
//! are obviously fake (`.agents/rules/tests.md`) except the RFC's own, which the vectors
//! need verbatim.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use api_model::{Auth, Body, DigestAlgorithm, DigestAuth, HttpRequest, ItemId, Method};
use futures::FutureExt;
use futures::future::BoxFuture;
use secrets::{MemorySecretStore, SecretKey};
use variables::{ScopeKind, ScopeStack};

use crate::client::SendInput;
use crate::error::TransportError;
use crate::request::{Header, ResolvedRequest};
use crate::response::{Response, ResponseBody};
use crate::transport::Transport;

/// RFC 7616 §3.9.1: the user, password, request-target and client nonce of the example.
pub(crate) const RFC_USERNAME: &str = "Mufasa";
pub(crate) const RFC_PASSWORD: &str = "Circle of Life";
pub(crate) const RFC_URI: &str = "/dir/index.html";
pub(crate) const RFC_CNONCE: &str = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";

/// RFC 7616 §3.9.1: the SHA-256 challenge, offered first.
pub(crate) const RFC_SHA256_CHALLENGE: &str = r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=SHA-256, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;

/// RFC 7616 §3.9.1: the MD5 challenge offered beside it.
pub(crate) const RFC_MD5_CHALLENGE: &str = r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm=MD5, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#;

/// The fake password the send tests use.
pub(crate) const PASSWORD: &str = "password-REDACTED-digest-0042";

/// The state lock. A poisoned lock still holds usable state for a test.
fn lock<T>(state: &Mutex<T>) -> MutexGuard<'_, T> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What [`ScriptedTransport`] records.
#[derive(Default)]
struct Script {
    responses: VecDeque<Response>,
    requests: Vec<ResolvedRequest>,
}

/// A [`Transport`] answering each send with the next scripted [`Response`] at once, and
/// failing loudly when the script ran out.
#[derive(Clone, Default)]
pub(crate) struct ScriptedTransport {
    script: Arc<Mutex<Script>>,
}

impl ScriptedTransport {
    pub(crate) fn new(responses: impl IntoIterator<Item = Response>) -> Self {
        let transport = Self::default();
        lock(&transport.script).responses.extend(responses);
        transport
    }

    /// The requests received, in order.
    pub(crate) fn requests(&self) -> Vec<ResolvedRequest> {
        lock(&self.script).requests.clone()
    }
}

impl Transport for ScriptedTransport {
    fn send(
        &self,
        request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let next = {
            let mut script = lock(&self.script);
            script.requests.push(request);
            script.responses.pop_front()
        };
        async move {
            next.ok_or_else(|| TransportError::Io {
                detail: "ScriptedTransport: no scripted response left".to_string(),
            })
        }
        .boxed()
    }
}

/// A response with `status`, these `WWW-Authenticate` fields and `body`.
pub(crate) fn response(status: u16, challenges: &[&str], body: &str) -> Response {
    let mut response = Response::new(status, "", ResponseBody::Bytes(body.as_bytes().to_vec()));
    response.headers = challenges
        .iter()
        .map(|challenge| Header::new("WWW-Authenticate", *challenge))
        .collect();
    response
}

/// A `method` request to `https://example.test/dir/index.html?page=1` with Digest auth
/// for `digest-user-example` and [`PASSWORD`] under `algorithm`, and `body`.
pub(crate) fn digest_request(
    method: Method,
    algorithm: DigestAlgorithm,
    body: Body,
) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "digest test".to_string(),
        method,
        url: "https://example.test/dir/index.html?page=1".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body,
        auth: Auth::Digest(DigestAuth {
            username: "digest-user-example".to_string(),
            password: PASSWORD.to_string(),
            algorithm,
            ..DigestAuth::default()
        }),
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// Everything one send of `request` needs: no variables, no keychain.
pub(crate) fn send_input(request: HttpRequest) -> SendInput {
    SendInput {
        request,
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(no_secret_key),
    }
}

fn no_secret_key(_kind: ScopeKind, _name: &str) -> Option<SecretKey> {
    None
}

/// The `Authorization` value `request` carried, if any.
pub(crate) fn authorization(request: &ResolvedRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("authorization"))
        .map(|header| header.value.as_str())
}

/// The value of parameter `name` in a Digest `Authorization` value.
pub(crate) fn parameter(authorization: &str, name: &str) -> Option<String> {
    http_auth::parse_challenges(authorization)
        .ok()?
        .first()?
        .params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.to_unescaped())
}
