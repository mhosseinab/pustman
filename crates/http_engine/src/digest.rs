//! Digest access authentication (RFC 7616, PRD REQ-HTTP-07): reading the challenges of a
//! 401's `WWW-Authenticate` fields and answering one with an `Authorization` value.
//!
//! The hashing and the header syntax are `http-auth`'s; this module decides which
//! challenge may be answered. It answers only the algorithm the user chose — never a
//! weaker one the server also offers — and only `qop=auth`: `auth-int` would hash a body
//! this engine cannot promise to replay byte for byte, and a challenge without a qop
//! (RFC 2069) carries no client nonce or nonce count.
//!
//! Pure: no IO and no logging. The password and the computed response are credentials,
//! and no `Debug` here prints either.

use std::fmt;

use api_model::{DigestAlgorithm, Method};
use http_auth::digest::{Algorithm, DigestClient, Qop};
use http_auth::{PasswordParams, parse_challenges};
use url::{Position, Url};

use crate::request::{Header, RequestBody};

/// The resolved credentials a Digest auth answers its challenge with. Nothing of them
/// goes on the wire before the server asks (RFC 7616 §3.3).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DigestCredentials {
    username: String,
    password: String,
    algorithm: DigestAlgorithm,
}

impl DigestCredentials {
    pub(crate) fn new(username: String, password: String, algorithm: DigestAlgorithm) -> Self {
        Self {
            username,
            password,
            algorithm,
        }
    }
}

impl fmt::Debug for DigestCredentials {
    /// The algorithm only: the username is an identity history masks too, and the
    /// password is a credential.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigestCredentials")
            .field("algorithm", &self.algorithm)
            .finish_non_exhaustive()
    }
}

/// One answered challenge.
pub(crate) struct DigestAnswer {
    /// The whole `Authorization` value: `Digest username=…, response="…", …`.
    pub(crate) authorization: String,
    /// The `response` hash alone, so a server echoing it in another shape is masked too.
    pub(crate) response: String,
}

impl fmt::Debug for DigestAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigestAnswer").finish_non_exhaustive()
    }
}

/// Why a 401 went back to the user unanswered. Each names what was not supported, never
/// a credential or a value the server sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DigestRefusal {
    /// No `WWW-Authenticate` field held a Digest challenge.
    NoChallenge,
    /// Digest challenges came, none with the algorithm the user chose. Answering another
    /// would be a silent downgrade (e.g. SHA-256 → MD5).
    AlgorithmNotOffered(DigestAlgorithm),
    /// The chosen algorithm's challenge offers only `qop=auth-int`.
    AuthIntOnly,
    /// The chosen algorithm's challenge names no qop (RFC 2069): no nonce count.
    NoQop,
    /// The body is a file or form-data, read again from disk on a retry: the second
    /// request could carry different bytes than the first.
    BodyNotReplayable,
    /// The 401 came after a redirect: it is another URL's challenge, possibly another
    /// origin's, and credentials never follow a redirect unasked.
    Redirected,
    /// `http-auth` could not build the header (a nonce count exhausted, a value it
    /// cannot quote).
    Unanswerable,
}

impl DigestRefusal {
    /// The auth "kind" the user is told is not supported, in
    /// `Warning::AuthUnsupported`'s sentence: `The "<kind>" auth type is not supported
    /// yet. The request was sent without credentials.`
    pub(crate) fn kind(self) -> String {
        match self {
            DigestRefusal::NoChallenge => "Digest without a Digest challenge".to_string(),
            DigestRefusal::AlgorithmNotOffered(algorithm) => format!(
                "Digest {} when the server does not offer it",
                algorithm.as_str()
            ),
            DigestRefusal::AuthIntOnly => "Digest with qop=auth-int".to_string(),
            DigestRefusal::NoQop => "Digest without qop (RFC 2069)".to_string(),
            DigestRefusal::BodyNotReplayable => "Digest with a file or form-data body".to_string(),
            DigestRefusal::Redirected => "Digest after a redirect".to_string(),
            DigestRefusal::Unanswerable => "Digest for this challenge".to_string(),
        }
    }
}

/// Whether a request with `body` can be sent again with the same bytes: only bytes held
/// in memory can. A file is read again, and a multipart body draws a new boundary.
pub(crate) fn body_is_replayable(body: &RequestBody) -> bool {
    matches!(body, RequestBody::None | RequestBody::Bytes(_))
}

/// The request-target the `uri` parameter must repeat (RFC 7616 §3.4): path and query.
pub(crate) fn request_uri(url: &Url) -> &str {
    &url[Position::BeforePath..Position::AfterQuery]
}

/// Answer the Digest challenge among `headers` (a 401's response headers) that matches
/// the user's algorithm, for a `method` request to `uri`, with a fresh client nonce.
pub(crate) fn answer(
    headers: &[Header],
    credentials: &DigestCredentials,
    method: &Method,
    uri: &str,
) -> Result<DigestAnswer, DigestRefusal> {
    answer_with(headers, credentials, method, uri, |client, params| {
        client.respond(params)
    })
}

/// [`answer`], with `respond` computing the header from the chosen challenge — the test
/// vectors fix the client nonce through it.
fn answer_with(
    headers: &[Header],
    credentials: &DigestCredentials,
    method: &Method,
    uri: &str,
    respond: impl Fn(&mut DigestClient, &PasswordParams) -> Result<String, String>,
) -> Result<DigestAnswer, DigestRefusal> {
    let wanted = algorithm_of(credentials.algorithm);
    let mut refusal = DigestRefusal::NoChallenge;
    let values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("www-authenticate"));
    for value in values {
        // A field that does not parse holds no challenge this send can answer.
        let Ok(challenges) = parse_challenges(&value.value) else {
            continue;
        };
        for challenge in challenges
            .iter()
            .filter(|challenge| challenge.scheme.eq_ignore_ascii_case("Digest"))
        {
            // A Digest challenge `http-auth` rejects names an algorithm it cannot do.
            let Ok(mut client) = DigestClient::try_from(challenge) else {
                refusal =
                    refusal.max_specific(DigestRefusal::AlgorithmNotOffered(credentials.algorithm));
                continue;
            };
            if (client.algorithm(), client.session()) != wanted {
                refusal =
                    refusal.max_specific(DigestRefusal::AlgorithmNotOffered(credentials.algorithm));
                continue;
            }
            if client.rfc2069_compat() {
                refusal = refusal.max_specific(DigestRefusal::NoQop);
                continue;
            }
            if !(client.qop() & Qop::Auth) {
                refusal = refusal.max_specific(DigestRefusal::AuthIntOnly);
                continue;
            }
            let params = PasswordParams {
                username: &credentials.username,
                password: &credentials.password,
                uri,
                method: method.as_str(),
                // No body: `qop=auth` hashes none, and `auth-int` is never chosen.
                body: None,
            };
            let authorization =
                respond(&mut client, &params).map_err(|_| DigestRefusal::Unanswerable)?;
            let response = response_of(&authorization).ok_or(DigestRefusal::Unanswerable)?;
            return Ok(DigestAnswer {
                authorization,
                response,
            });
        }
    }
    Err(refusal)
}

impl DigestRefusal {
    /// The more telling of two refusals for one 401: the closer a challenge came to being
    /// answered, the more the user learns from its reason.
    fn max_specific(self, other: DigestRefusal) -> DigestRefusal {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    fn rank(self) -> u8 {
        match self {
            DigestRefusal::NoChallenge => 0,
            DigestRefusal::AlgorithmNotOffered(_) => 1,
            DigestRefusal::NoQop | DigestRefusal::AuthIntOnly => 2,
            DigestRefusal::BodyNotReplayable
            | DigestRefusal::Redirected
            | DigestRefusal::Unanswerable => 3,
        }
    }
}

/// The `(algorithm, session)` pair `http-auth` reports for the user's choice.
fn algorithm_of(algorithm: DigestAlgorithm) -> (Algorithm, bool) {
    match algorithm {
        DigestAlgorithm::Md5 => (Algorithm::Md5, false),
        DigestAlgorithm::Md5Sess => (Algorithm::Md5, true),
        DigestAlgorithm::Sha256 => (Algorithm::Sha256, false),
        DigestAlgorithm::Sha256Sess => (Algorithm::Sha256, true),
    }
}

/// The `response` parameter of an `Authorization` value `http-auth` wrote. Its syntax is
/// a challenge's (`Digest k=v, …`), so the same parser reads it back.
fn response_of(authorization: &str) -> Option<String> {
    let parsed = parse_challenges(authorization).ok()?;
    parsed.first().and_then(|credentials| {
        credentials
            .params
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("response"))
            .map(|(_, value)| value.to_unescaped())
    })
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "digest_test_support.rs"]
pub(crate) mod digest_test_support;
