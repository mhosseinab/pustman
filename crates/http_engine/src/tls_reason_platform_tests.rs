//! The platform verifier's name-mismatch wording. On macOS, rustls-platform-verifier maps
//! `errSecHostNameMismatch` to the bare `CertificateError::NotValidForName`, which renders
//! as its variant name rather than the webpki sentence; the user must still read the same
//! reason on every platform.

use super::tls_reason;

#[test]
fn test_a_platform_name_mismatch_reads_like_the_webpki_one() {
    assert_eq!(
        tls_reason("invalid peer certificate: NotValidForName"),
        "invalid peer certificate: certificate not valid for name"
    );
}

#[test]
fn test_the_webpki_name_mismatch_wording_is_unchanged() {
    assert_eq!(
        tls_reason(
            "invalid peer certificate: certificate not valid for name \"token-REDACTED-example.example.test\"; certificate is only valid for DnsName(\"localhost\")"
        ),
        "invalid peer certificate: certificate not valid for name"
    );
}
