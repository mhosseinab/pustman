//! `MethodError`'s wording cites the same RFC as the token rule it enforces (LOW #117).

use super::MethodError;

#[test]
fn test_method_error_message_cites_the_same_rfc_as_the_token_rule() {
    let message = MethodError.to_string();
    assert!(
        message.contains("RFC 9110"),
        "MethodError must cite RFC 9110, the RFC the token rule is documented against; got: {message}"
    );
    assert!(
        !message.contains("RFC 7230"),
        "MethodError must not cite the obsoleted RFC 7230; got: {message}"
    );
}
