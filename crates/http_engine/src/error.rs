//! Why a send failed, and what to tell the user about it (PRD §8.6).

use std::fmt;
use std::time::Duration;

use crate::response::RedirectHop;

/// A transport failure, exactly the PRD §8.6 set. 4xx and 5xx are not failures: they
/// arrive as [`crate::Response`]s and render normally.
///
/// Every kind carries the text §8.6 prescribes through [`TransportError::user_message`]
/// and [`TransportError::suggested_fix`].
///
/// The log contract: error formatting goes through `Display` (`{error:#}`) and
/// [`Debug`], so **both** are log-safe — the resolved URL, host, address and path can
/// carry secret values (PRD §7) and neither trait prints them. [`Debug`] is
/// hand-written; `Display` is redacted at the `#[error(...)]` attributes. The full
/// sentences live only in [`TransportError::user_message`], which is for the UI and is
/// never logged.
#[derive(Clone, PartialEq, thiserror::Error)]
pub enum TransportError {
    /// The resolved URL is not a valid URL.
    #[error("Invalid URL after variable resolution.")]
    InvalidUrl { url: String },

    /// The host name did not resolve.
    #[error("Could not resolve the host.")]
    Dns { host: String },

    /// The connection could not be established.
    #[error("Connection refused.")]
    Connect { address: String },

    /// The TLS handshake or the certificate check failed.
    #[error("{reason}")]
    Tls { reason: String },

    /// No response within the request timeout.
    #[error("No response within {} s (request timeout).", .after.as_secs())]
    Timeout { after: Duration },

    /// Redirects kept coming past the limit; the chain shows every hop.
    #[error("Stopped after {} redirects.", .chain.len())]
    TooManyRedirects { chain: Vec<RedirectHop> },

    /// The proxy refused or mis-handled the request.
    #[error("{reason}")]
    Proxy { reason: String },

    /// The server spoke a protocol the request could not continue with.
    #[error("{detail}")]
    Protocol { detail: String },

    /// A transport-level I/O failure.
    #[error("{detail}")]
    Io { detail: String },

    /// A body file does not exist (REQ-HTTP-05).
    #[error("Body file not found.")]
    FileNotFound { path: String },

    /// A secret variable has no local value, so the send was refused rather than putting
    /// an empty string on the wire (REQ-VAR-SHARE-02).
    #[error("{}", unresolved_secret_message(.names))]
    UnresolvedSecret { names: Vec<String> },

    /// The user cancelled the send (REQ-HTTP-12).
    #[error("Cancelled by user.")]
    Cancelled,
}

impl fmt::Debug for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The resolved URL, host, address and path carry resolved text (PRD §7), and
        // error logging formats `Debug` alongside `Display`: redact those four, keep
        // the rest.
        match self {
            TransportError::InvalidUrl { .. } => f
                .debug_struct("InvalidUrl")
                .field("url", &"<redacted>")
                .finish(),
            TransportError::Dns { .. } => {
                f.debug_struct("Dns").field("host", &"<redacted>").finish()
            }
            TransportError::Connect { .. } => f
                .debug_struct("Connect")
                .field("address", &"<redacted>")
                .finish(),
            TransportError::Tls { reason } => {
                f.debug_struct("Tls").field("reason", reason).finish()
            }
            TransportError::Timeout { after } => {
                f.debug_struct("Timeout").field("after", after).finish()
            }
            TransportError::TooManyRedirects { chain } => f
                .debug_struct("TooManyRedirects")
                .field("chain", chain)
                .finish(),
            TransportError::Proxy { reason } => {
                f.debug_struct("Proxy").field("reason", reason).finish()
            }
            TransportError::Protocol { detail } => {
                f.debug_struct("Protocol").field("detail", detail).finish()
            }
            TransportError::Io { detail } => f.debug_struct("Io").field("detail", detail).finish(),
            TransportError::FileNotFound { .. } => f
                .debug_struct("FileNotFound")
                .field("path", &"<redacted>")
                .finish(),
            TransportError::UnresolvedSecret { names } => f
                .debug_struct("UnresolvedSecret")
                .field("names", names)
                .finish(),
            TransportError::Cancelled => f.write_str("Cancelled"),
        }
    }
}

/// §8.6: "Secret `api_key` has no local value." — one message per unresolved name set.
fn unresolved_secret_message(names: &[String]) -> String {
    match names {
        [] => "A secret has no local value.".to_string(),
        [name] => format!("Secret `{name}` has no local value."),
        many => format!("Secrets `{}` have no local value.", many.join("`, `")),
    }
}

/// The reason sentence-cased: the transport's raw wording arrives lowercase from the
/// TLS stack ("invalid peer certificate: …"), and the message the user reads must start
/// like a sentence (LOW #228). Only the first letter changes — the reason is the
/// transport's own text and the copy tests pin it verbatim otherwise.
fn sentence_case(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

impl TransportError {
    /// The message shown to the user (PRD §8.6). This is the only text that carries
    /// resolved values: show it in the UI, never log it.
    pub fn user_message(&self) -> String {
        // The four sensitive variants print their full sentence here and only here;
        // every other kind's sentence lives in its `#[error(...)]` attribute and comes
        // back through `to_string()` (Display is log-safe, see the enum docs). `Tls`
        // goes through `sentence_case` instead of the fallthrough: its reason is the
        // transport's raw lowercase wording (LOW #228).
        match self {
            TransportError::InvalidUrl { url } => {
                format!("Invalid URL after variable resolution: `{url}`")
            }
            TransportError::Dns { host } => format!("Could not resolve `{host}`."),
            TransportError::Connect { address } => format!("Connection refused at {address}."),
            TransportError::Tls { reason } => sentence_case(reason),
            TransportError::FileNotFound { path } => format!("Body file not found: {path}"),
            other => other.to_string(),
        }
    }

    /// What the user can do about it (PRD §8.6).
    pub fn suggested_fix(&self) -> &'static str {
        match self {
            TransportError::InvalidUrl { .. } => {
                "Replace any unset `{{variable}}` in the URL with a value, or correct the URL."
            }
            TransportError::Dns { .. } => "Check the host name, VPN, or proxy settings.",
            TransportError::Connect { .. } => "Is the server running?",
            TransportError::Tls { .. } => {
                "Add a CA certificate or disable verification for this request."
            }
            TransportError::Timeout { .. } => "Change the timeout in request settings.",
            TransportError::TooManyRedirects { .. } => {
                "Raise the redirect limit in request settings, or turn redirects off."
            }
            TransportError::Proxy { .. } => "Check the proxy settings and credentials.",
            TransportError::Protocol { .. } => {
                "Check which protocols the server supports, or pick another HTTP version in request settings."
            }
            TransportError::Io { .. } => "Check the file path and permissions, then try again.",
            TransportError::FileNotFound { .. } => {
                "Check the path in the body editor; a relative path starts at the workspace root."
            }
            TransportError::UnresolvedSecret { .. } => {
                "Set a local value in the active environment."
            }
            TransportError::Cancelled => "Send the request again when you are ready.",
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "error_copy_tests.rs"]
mod copy_tests;

#[cfg(test)]
#[path = "error_sentence_case_tests.rs"]
mod sentence_case_tests;
