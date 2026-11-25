//! One rustls crypto provider for the whole process.
//!
//! tonic's client TLS takes the process-default rustls provider when one is installed
//! and otherwise picks one per build feature, so without an installed default a gRPC
//! channel and reqwest (aws-lc-rs) could disagree once another crate enables rustls'
//! `ring` feature. Every channel installs aws-lc-rs — the provider reqwest uses — before
//! it builds a TLS config.

use std::sync::Once;

/// Install the aws-lc-rs provider as the process default, once. Later calls, and calls
/// after something else installed a default, change nothing.
pub fn ensure_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // `Err` means a default was already installed (by `websocket_client` or the
        // app); keeping it is right — replacing a provider mid-process is not possible.
        if rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .is_err()
        {
            log::debug!("a rustls crypto provider was already installed; keeping it");
        }
    });
}

#[cfg(test)]
#[path = "tls_provider_tests.rs"]
mod tests;
