use super::*;

/// Obviously fake, holding characters every percent-encoder escapes.
const SECRET: &str = "ab+c/d=e";

/// Some servers echo percent-escapes in lowercase hex (`HttpUtility.UrlEncode` and other
/// legacy encoders): the secret must be masked in that spelling too (S38 R1 review, LOW 1).
#[test]
fn a_secret_is_masked_when_echoed_with_lowercase_percent_hex() {
    let secrets = SecretValues::from_values([SECRET]);
    for echoed in ["ab%2bc%2fd%3de", "ab+c%2fd=e"] {
        let text = format!("before {echoed} after");
        assert_eq!(
            redact_text(&text, &secrets),
            format!("before {MASK} after"),
            "the lowercase-hex echo {echoed:?} must be masked"
        );
    }
}
