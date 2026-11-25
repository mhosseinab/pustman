//! Network settings (PRD REQ-HTTP-10, REQ-HTTP-11): the outgoing proxy, client certificates
//! per host and a custom CA bundle.
//!
//! These are plain serde types: nothing here reads a file, parses key material or touches
//! the keychain. Credentials — the proxy's username and password, a PFX passphrase — are
//! never stored in these types, only a [`SecretRef`] naming where the keychain keeps them,
//! so the settings file can be shared or committed without leaking one.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Names a secret held in the OS keychain. It is the name, never the value: resolving it
/// is the caller's job, at send time.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Refer to the keychain entry called `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The keychain entry's name.
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// The protocol spoken to a manual proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyScheme {
    Http,
    Https,
    Socks5,
}

/// Where requests go before they reach the server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ProxyMode {
    /// Use the operating system's proxy configuration.
    #[default]
    System,
    /// Use the proxy the user typed in.
    Manual {
        scheme: ProxyScheme,
        host: String,
        port: u16,
        /// The keychain entry holding the proxy's credentials, when it needs any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_secret: Option<SecretRef>,
    },
    /// Connect directly.
    None,
}

/// The outgoing proxy and the hosts that skip it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(flatten)]
    pub mode: ProxyMode,
    /// `NO_PROXY` entries: a host matches an entry it equals or is a subdomain of. A
    /// leading `.` or `*.` on an entry is optional, and `*` alone matches every host.
    #[serde(default)]
    pub no_proxy: Vec<String>,
}

impl ProxyConfig {
    /// Whether `host` is listed in [`ProxyConfig::no_proxy`] and so connects directly.
    /// Hosts and entries compare case-insensitively, ignoring a trailing `.`.
    pub fn matches_no_proxy(&self, host: &str) -> bool {
        let host = normalize_host(host);
        if host.is_empty() {
            return false;
        }
        self.no_proxy.iter().any(|entry| {
            let entry = entry.trim();
            if entry == "*" {
                return true;
            }
            let domain = entry
                .strip_prefix("*.")
                .or_else(|| entry.strip_prefix('.'))
                .unwrap_or(entry);
            let domain = normalize_host(domain);
            !domain.is_empty() && is_same_or_subdomain(&host, &domain)
        })
    }
}

/// The file format of a client certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClientCertificateKind {
    /// A PEM certificate chain and its private key, in two files.
    Pem {
        cert_path: PathBuf,
        key_path: PathBuf,
    },
    /// A PKCS#12 archive holding both. Its passphrase lives in the keychain.
    Pfx {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase_secret: Option<SecretRef>,
    },
}

/// A client certificate presented to the hosts its pattern covers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCertificate {
    /// `api.example.com` covers that host alone; `*.example.com` covers its subdomains at
    /// any depth but not `example.com` itself.
    pub host_pattern: String,
    #[serde(flatten)]
    pub kind: ClientCertificateKind,
}

impl ClientCertificate {
    /// How specifically this certificate's pattern covers `host` (already normalized), or
    /// `None` when it does not cover it. An exact pattern outranks every wildcard, and a
    /// longer wildcard outranks a shorter one.
    fn specificity(&self, host: &str) -> Option<(bool, usize)> {
        let pattern = normalize_host(&self.host_pattern);
        match pattern.strip_prefix("*.") {
            Some(domain) => {
                let covered = !domain.is_empty()
                    && host.len() > domain.len()
                    && is_same_or_subdomain(host, domain);
                covered.then_some((false, domain.len()))
            }
            None => (!pattern.is_empty() && pattern == host).then_some((true, pattern.len())),
        }
    }
}

/// Every network setting (the `network` section of the settings file).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettings {
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub client_certificates: Vec<ClientCertificate>,
    /// Extra certificate authorities trusted on top of the system's, as a PEM file.
    #[serde(default)]
    pub ca_bundle_path: Option<PathBuf>,
}

impl NetworkSettings {
    /// The client certificate to present to `host`: the one whose pattern covers it most
    /// specifically, and the earliest listed among equals.
    pub fn certificate_for(&self, host: &str) -> Option<&ClientCertificate> {
        let host = normalize_host(host);
        if host.is_empty() {
            return None;
        }
        let mut best: Option<(&ClientCertificate, (bool, usize))> = None;
        for certificate in &self.client_certificates {
            if let Some(rank) = certificate.specificity(&host)
                && best.is_none_or(|(_, best_rank)| rank > best_rank)
            {
                best = Some((certificate, rank));
            }
        }
        best.map(|(certificate, _)| certificate)
    }
}

/// A host name as compared: trimmed, lowercase, without a trailing root `.`.
fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Whether `host` is `domain` or ends in `.domain`, so a suffix only matches on a label
/// boundary (`badexample.com` is not under `example.com`).
fn is_same_or_subdomain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
