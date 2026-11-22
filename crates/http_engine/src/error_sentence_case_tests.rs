//! Copy check for the TLS message the UI shows (LOW #228): the transport's raw reason
//! strings arrive lowercase from rustls, and the message the user reads must be a
//! sentence like every other error's.

use crate::TransportError;

#[test]
fn test_a_lowercase_tls_reason_reaches_the_user_sentence_cased() {
    let error = TransportError::Tls {
        reason: String::from("invalid peer certificate: UnknownIssuer"),
    };

    assert_eq!(
        error.user_message(),
        "Invalid peer certificate: UnknownIssuer",
        "the raw lowercase reason must be sentence-cased for the UI (LOW #228)"
    );
}

#[test]
fn test_an_already_sentence_cased_tls_reason_is_shown_unchanged() {
    let error = TransportError::Tls {
        reason: String::from("Certificate not trusted (self-signed)."),
    };

    assert_eq!(
        error.user_message(),
        "Certificate not trusted (self-signed).",
        "sentence-casing must not re-write a reason that already reads as a sentence"
    );
}
