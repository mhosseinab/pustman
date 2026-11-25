//! The auth kinds beyond Basic, Bearer and API key: Digest, OAuth 2.0, AWS Signature v4 and
//! JWT Bearer (PRD REQ-HTTP-06/07).
//!
//! These are plain data: nothing here signs, hashes or fetches a token. A credential field
//! holds a value or a `{{variable}}` reference exactly as Basic's password does, and an OAuth
//! 2.0 token is never a field at all — only a [`SecretRef`] naming the keychain entry that
//! holds it. `Debug` masks every credential so a logged model never shows one.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::{ApiKeyLocation, Auth, MASK, SecretRef};

/// The hash a Digest challenge names (RFC 7616 §3.3); the `-sess` forms hash the session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DigestAlgorithm {
    #[default]
    #[serde(rename = "MD5")]
    Md5,
    #[serde(rename = "MD5-sess")]
    Md5Sess,
    #[serde(rename = "SHA-256")]
    Sha256,
    #[serde(rename = "SHA-256-sess")]
    Sha256Sess,
}

impl DigestAlgorithm {
    const ALL: [DigestAlgorithm; 4] = [
        DigestAlgorithm::Md5,
        DigestAlgorithm::Md5Sess,
        DigestAlgorithm::Sha256,
        DigestAlgorithm::Sha256Sess,
    ];

    /// The token as RFC 7616 and Postman spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            DigestAlgorithm::Md5 => "MD5",
            DigestAlgorithm::Md5Sess => "MD5-sess",
            DigestAlgorithm::Sha256 => "SHA-256",
            DigestAlgorithm::Sha256Sess => "SHA-256-sess",
        }
    }

    /// Read a token; the RFC makes the comparison case-insensitive.
    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|algorithm| algorithm.as_str().eq_ignore_ascii_case(token.trim()))
    }
}

/// HTTP Digest authentication (RFC 7616). `realm`, `nonce` and `qop` normally come from the
/// server's challenge; a value here pre-fills it.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DigestAuth {
    pub username: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub algorithm: DigestAlgorithm,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qop: Option<String>,
    /// Fields the model does not know. Serialized by `Auth`, after the typed fields and
    /// never over them (see `ExtendedKind::FIELDS`).
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// How an OAuth 2.0 client obtains its token (REQ-HTTP-07).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OAuth2Grant {
    /// The browser flow; PKCE is on unless the user turned it off.
    AuthorizationCode {
        #[serde(default)]
        auth_url: String,
        #[serde(default = "pkce_by_default")]
        use_pkce: bool,
    },
    ClientCredentials,
    /// Resource owner password credentials.
    Password {
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    /// Kept for imported collections only: OAuth 2.0 Security BCP deprecates it.
    Implicit {
        #[serde(default)]
        auth_url: String,
    },
}

fn pkce_by_default() -> bool {
    true
}

impl Default for OAuth2Grant {
    fn default() -> Self {
        OAuth2Grant::AuthorizationCode {
            auth_url: String::new(),
            use_pkce: pkce_by_default(),
        }
    }
}

impl OAuth2Grant {
    /// Whether the UI must flag this grant as legacy (REQ-HTTP-07: implicit).
    pub fn is_legacy(&self) -> bool {
        matches!(self, OAuth2Grant::Implicit { .. })
    }
}

impl fmt::Debug for OAuth2Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuth2Grant::AuthorizationCode { auth_url, use_pkce } => f
                .debug_struct("AuthorizationCode")
                .field("auth_url", auth_url)
                .field("use_pkce", use_pkce)
                .finish(),
            OAuth2Grant::ClientCredentials => f.write_str("ClientCredentials"),
            OAuth2Grant::Password { username, password } => f
                .debug_struct("Password")
                .field("username", username)
                .field("password", &masked(password))
                .finish(),
            OAuth2Grant::Implicit { auth_url } => f
                .debug_struct("Implicit")
                .field("auth_url", auth_url)
                .field("legacy", &true)
                .finish(),
        }
    }
}

/// Where the client id and secret go on the token request (RFC 6749 §2.3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuthentication {
    /// An `Authorization: Basic` header.
    #[default]
    Header,
    /// `client_id` and `client_secret` form fields.
    Body,
}

/// OAuth 2.0 (REQ-HTTP-07). The token itself is never part of the model: `token_ref` names
/// the keychain entry that holds it once one was obtained.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OAuth2Auth {
    pub grant: OAuth2Grant,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    pub client_authentication: ClientAuthentication,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_ref: Option<SecretRef>,
    /// Fields the model does not know. Serialized by `Auth`, after the typed fields and
    /// never over them (see `ExtendedKind::FIELDS`).
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// AWS Signature Version 4.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AwsSigV4Auth {
    pub access_key: String,
    pub secret_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    pub region: String,
    pub service: String,
    /// Fields the model does not know. Serialized by `Auth`, after the typed fields and
    /// never over them (see `ExtendedKind::FIELDS`).
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The JWS algorithm a JWT Bearer token is signed with: HMAC with a shared secret, or RSA /
/// ECDSA with a private key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithm {
    #[default]
    Hs256,
    Hs384,
    Hs512,
    Rs256,
    Rs384,
    Rs512,
    Es256,
    Es384,
}

impl JwtAlgorithm {
    const ALL: [JwtAlgorithm; 8] = [
        JwtAlgorithm::Hs256,
        JwtAlgorithm::Hs384,
        JwtAlgorithm::Hs512,
        JwtAlgorithm::Rs256,
        JwtAlgorithm::Rs384,
        JwtAlgorithm::Rs512,
        JwtAlgorithm::Es256,
        JwtAlgorithm::Es384,
    ];

    /// The JWS `alg` name.
    pub fn as_str(self) -> &'static str {
        match self {
            JwtAlgorithm::Hs256 => "HS256",
            JwtAlgorithm::Hs384 => "HS384",
            JwtAlgorithm::Hs512 => "HS512",
            JwtAlgorithm::Rs256 => "RS256",
            JwtAlgorithm::Rs384 => "RS384",
            JwtAlgorithm::Rs512 => "RS512",
            JwtAlgorithm::Es256 => "ES256",
            JwtAlgorithm::Es384 => "ES384",
        }
    }

    /// Read a JWS `alg` name. Names are case-sensitive (RFC 7515 §4.1.1).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|algorithm| algorithm.as_str() == name)
    }

    /// Whether the key is a shared secret rather than a private key.
    pub fn is_hmac(self) -> bool {
        matches!(
            self,
            JwtAlgorithm::Hs256 | JwtAlgorithm::Hs384 | JwtAlgorithm::Hs512
        )
    }
}

/// A JWT signed at send time from `payload` and sent as a bearer token.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JwtBearerAuth {
    pub algorithm: JwtAlgorithm,
    /// The HMAC secret or the PEM private key, by `algorithm`.
    pub secret_or_key: String,
    /// The claims, as JSON text.
    pub payload: String,
    pub header_prefix: String,
    pub add_to: ApiKeyLocation,
    /// Fields the model does not know. Serialized by `Auth`, after the typed fields and
    /// never over them (see `ExtendedKind::FIELDS`).
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for JwtBearerAuth {
    fn default() -> Self {
        Self {
            algorithm: JwtAlgorithm::default(),
            secret_or_key: String::new(),
            payload: String::new(),
            header_prefix: "Bearer".to_owned(),
            add_to: ApiKeyLocation::Header,
            extra: BTreeMap::new(),
        }
    }
}

/// What `Auth` needs to write one of these kinds and to find their credentials. `extra` is
/// written after the typed fields and may never shadow one, set or not: an imported `extra`
/// entry named `token_ref` would otherwise read back as the real reference and point the
/// token lookup at another keychain entry.
pub(crate) trait ExtendedKind: Serialize {
    /// Every typed field's JSON name.
    const FIELDS: &'static [&'static str];

    /// The Postman fields this kind keeps in `extra` that are known to hold no
    /// credential. Every other `extra` entry is treated as one: Postman keeps real
    /// credentials in fields the model does not use (an unused JWT `privateKey`, a
    /// password left behind by a grant switch, a `client_secret` token parameter).
    const HARMLESS_EXTRA: &'static [&'static str];

    fn extra(&self) -> &BTreeMap<String, serde_json::Value>;

    /// The typed secret fields, and `extra`.
    fn credentials_mut(&mut self) -> (Vec<&mut String>, &mut BTreeMap<String, serde_json::Value>);

    /// The typed identity fields: not a secret, so the import report and the export warning
    /// ignore them, but history masks them like Basic's username.
    fn identities_mut(&mut self) -> Vec<&mut String> {
        Vec::new()
    }
}

impl ExtendedKind for DigestAuth {
    const FIELDS: &'static [&'static str] =
        &["username", "password", "realm", "nonce", "algorithm", "qop"];
    const HARMLESS_EXTRA: &'static [&'static str] =
        &["opaque", "nonceCount", "clientNonce", "disableRetryRequest"];

    fn extra(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extra
    }

    fn credentials_mut(&mut self) -> (Vec<&mut String>, &mut BTreeMap<String, serde_json::Value>) {
        (vec![&mut self.password], &mut self.extra)
    }

    fn identities_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.username]
    }
}

impl ExtendedKind for OAuth2Auth {
    const FIELDS: &'static [&'static str] = &[
        "grant",
        "token_url",
        "client_id",
        "client_secret",
        "scope",
        "audience",
        "client_authentication",
        "token_ref",
    ];
    const HARMLESS_EXTRA: &'static [&'static str] = &[
        "tokenName",
        "challengeAlgorithm",
        "addTokenTo",
        "headerPrefix",
        "redirect_uri",
        "useBrowser",
        "tokenType",
        "state",
        "refreshTokenUrl",
    ];

    fn extra(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extra
    }

    fn credentials_mut(&mut self) -> (Vec<&mut String>, &mut BTreeMap<String, serde_json::Value>) {
        let mut secrets = vec![&mut self.client_secret];
        if let OAuth2Grant::Password { password, .. } = &mut self.grant {
            secrets.push(password);
        }
        (secrets, &mut self.extra)
    }

    fn identities_mut(&mut self) -> Vec<&mut String> {
        match &mut self.grant {
            OAuth2Grant::Password { username, .. } => vec![username],
            OAuth2Grant::AuthorizationCode { .. }
            | OAuth2Grant::ClientCredentials
            | OAuth2Grant::Implicit { .. } => Vec::new(),
        }
    }
}

impl ExtendedKind for AwsSigV4Auth {
    const FIELDS: &'static [&'static str] = &[
        "access_key",
        "secret_key",
        "session_token",
        "region",
        "service",
    ];
    const HARMLESS_EXTRA: &'static [&'static str] = &["addAuthDataToQuery"];

    fn extra(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extra
    }

    fn credentials_mut(&mut self) -> (Vec<&mut String>, &mut BTreeMap<String, serde_json::Value>) {
        let mut secrets = vec![&mut self.secret_key];
        secrets.extend(self.session_token.as_mut());
        (secrets, &mut self.extra)
    }

    fn identities_mut(&mut self) -> Vec<&mut String> {
        vec![&mut self.access_key]
    }
}

impl ExtendedKind for JwtBearerAuth {
    const FIELDS: &'static [&'static str] = &[
        "algorithm",
        "secret_or_key",
        "payload",
        "header_prefix",
        "add_to",
    ];
    const HARMLESS_EXTRA: &'static [&'static str] =
        &["queryParamKey", "isSecretBase64Encoded", "header"];

    fn extra(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extra
    }

    fn credentials_mut(&mut self) -> (Vec<&mut String>, &mut BTreeMap<String, serde_json::Value>) {
        (vec![&mut self.secret_or_key], &mut self.extra)
    }
}

/// Visit `kind`'s secret fields and every string under an `extra` entry not known to be
/// harmless.
fn visit_credentials<K: ExtendedKind>(kind: &mut K, visit: &mut dyn FnMut(&mut String)) {
    let (secrets, extra) = kind.credentials_mut();
    for secret in secrets {
        visit(secret);
    }
    for (key, value) in extra.iter_mut() {
        if !K::HARMLESS_EXTRA.contains(&key.as_str()) {
            visit_strings(value, visit);
        }
    }
}

fn visit_strings(value: &mut serde_json::Value, visit: &mut dyn FnMut(&mut String)) {
    match value {
        serde_json::Value::String(text) => visit(text),
        serde_json::Value::Array(items) => {
            for item in items {
                visit_strings(item, visit);
            }
        }
        serde_json::Value::Object(fields) => {
            for field in fields.values_mut() {
                visit_strings(field, visit);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Postman's OAuth 2.0 extra-parameter lists: `{key, value}` rows the user adds to the
/// authorization, token and refresh requests. A row is a credential by its name, the way a
/// header row is, so the import report and the export warning judge it by its key.
const OAUTH2_PARAMETER_LISTS: [&str; 3] = [
    "authRequestParams",
    "tokenRequestParams",
    "refreshRequestParams",
];

/// One extra-parameter row as `(key, value)`, when it has that shape: a string key and a
/// scalar or absent value. Any other entry is not a row and is scanned string by string.
fn parameter_row(entry: &serde_json::Value) -> Option<(String, Option<String>)> {
    let row = entry.as_object()?;
    let key = row.get("key")?.as_str()?.to_owned();
    let value = match row.get("value") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        Some(serde_json::Value::Bool(flag)) => Some(flag.to_string()),
        Some(serde_json::Value::Array(_) | serde_json::Value::Object(_)) => return None,
    };
    Some((key, value))
}

impl Auth {
    /// Visit every credential string of a Digest, OAuth 2.0, AWS SigV4 or JWT Bearer auth:
    /// its secret fields and, conservatively, every string under an `extra` entry not known
    /// to be harmless. History masks through this. Returns `false`, having visited nothing,
    /// for the other kinds, whose callers check their own fields.
    pub fn visit_extended_credentials(&mut self, mut visit: impl FnMut(&mut String)) -> bool {
        match self {
            Auth::Digest(kind) => visit_credentials(kind, &mut visit),
            Auth::OAuth2(kind) => visit_credentials(kind, &mut visit),
            Auth::AwsSigV4(kind) => visit_credentials(kind, &mut visit),
            Auth::JwtBearer(kind) => visit_credentials(kind, &mut visit),
            Auth::Inherit
            | Auth::None
            | Auth::Basic { .. }
            | Auth::Bearer { .. }
            | Auth::ApiKey { .. }
            | Auth::Unsupported(_) => return false,
        }
        true
    }

    /// Visit the identity fields of the same kinds (Digest and password-grant usernames,
    /// the AWS access key): history masks them beside the credentials, as it masks Basic's
    /// username. Visits nothing for any other kind.
    pub fn visit_history_identities(&mut self, mut visit: impl FnMut(&mut String)) {
        let identities = match self {
            Auth::Digest(kind) => kind.identities_mut(),
            Auth::OAuth2(kind) => kind.identities_mut(),
            Auth::AwsSigV4(kind) => kind.identities_mut(),
            Auth::JwtBearer(kind) => kind.identities_mut(),
            Auth::Inherit
            | Auth::None
            | Auth::Basic { .. }
            | Auth::Bearer { .. }
            | Auth::ApiKey { .. }
            | Auth::Unsupported(_) => Vec::new(),
        };
        for identity in identities {
            visit(identity);
        }
    }

    /// What the import report and the export warning check: the strings
    /// [`Auth::visit_extended_credentials`] visits, except the OAuth 2.0 extra-parameter
    /// rows, which [`Auth::extended_parameter_rows`] hands over to be judged by name.
    /// `None` for the kinds it does not cover.
    pub fn extended_credentials(&self) -> Option<Vec<String>> {
        let mut copy = self.clone();
        if let Auth::OAuth2(oauth2) = &mut copy {
            for list in OAUTH2_PARAMETER_LISTS {
                if let Some(serde_json::Value::Array(entries)) = oauth2.extra.get_mut(list) {
                    entries.retain(|entry| parameter_row(entry).is_none());
                }
            }
        }
        let mut values = Vec::new();
        copy.visit_extended_credentials(|text| values.push(text.clone()))
            .then_some(values)
    }

    /// The OAuth 2.0 extra-parameter rows as `(key, value)`, for a caller to judge by
    /// name. Empty for every other kind.
    pub fn extended_parameter_rows(&self) -> Vec<(String, Option<String>)> {
        let Auth::OAuth2(oauth2) = self else {
            return Vec::new();
        };
        OAUTH2_PARAMETER_LISTS
            .iter()
            .filter_map(|list| oauth2.extra.get(*list))
            .filter_map(serde_json::Value::as_array)
            .flatten()
            .filter_map(parameter_row)
            .collect()
    }
}

/// A credential as `Debug` shows it: whether one is set, never what it is.
fn masked(value: &str) -> &'static str {
    if value.is_empty() { "" } else { MASK }
}

fn masked_option(value: &Option<String>) -> Option<&'static str> {
    value.as_deref().map(masked)
}

/// `extra` as `Debug` shows it: the keys only, because an imported auth can carry a
/// credential under a key the model does not know.
fn extra_keys(extra: &BTreeMap<String, serde_json::Value>) -> Vec<&str> {
    extra.keys().map(String::as_str).collect()
}

impl fmt::Debug for DigestAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigestAuth")
            .field("username", &self.username)
            .field("password", &masked(&self.password))
            .field("realm", &self.realm)
            .field("nonce", &self.nonce)
            .field("algorithm", &self.algorithm)
            .field("qop", &self.qop)
            .field("extra", &extra_keys(&self.extra))
            .finish()
    }
}

impl fmt::Debug for OAuth2Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2Auth")
            .field("grant", &self.grant)
            .field("token_url", &self.token_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &masked(&self.client_secret))
            .field("scope", &self.scope)
            .field("audience", &self.audience)
            .field("client_authentication", &self.client_authentication)
            .field("token_ref", &self.token_ref)
            .field("extra", &extra_keys(&self.extra))
            .finish()
    }
}

impl fmt::Debug for AwsSigV4Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AwsSigV4Auth")
            .field("access_key", &self.access_key)
            .field("secret_key", &masked(&self.secret_key))
            .field("session_token", &masked_option(&self.session_token))
            .field("region", &self.region)
            .field("service", &self.service)
            .field("extra", &extra_keys(&self.extra))
            .finish()
    }
}

impl fmt::Debug for JwtBearerAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JwtBearerAuth")
            .field("algorithm", &self.algorithm)
            .field("secret_or_key", &masked(&self.secret_or_key))
            .field("payload", &masked(&self.payload))
            .field("header_prefix", &self.header_prefix)
            .field("add_to", &self.add_to)
            .field("extra", &extra_keys(&self.extra))
            .finish()
    }
}

#[cfg(test)]
#[path = "auth_extended_tests.rs"]
mod tests;
