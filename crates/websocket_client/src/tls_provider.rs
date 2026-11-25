//! One rustls crypto provider for the whole process.
//!
//! reqwest's `rustls` feature uses aws-lc-rs. rustls picks a process default from its
//! crate features only while exactly one provider is compiled in; once any dependency
//! adds a second (rustls' `ring` feature), `rustls::ClientConfig::builder()` panics. So
//! every session installs aws-lc-rs — the provider reqwest uses — before it builds a TLS
//! config, instead of relying on feature selection.

use std::sync::Once;

/// Install the aws-lc-rs provider as the process default, once. Later calls, and calls
/// after something else installed a default, change nothing.
pub fn ensure_crypto_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // `Err` means a default was already installed (by `grpc_client` or the app);
        // keeping it is right — replacing a provider mid-process is not possible.
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
