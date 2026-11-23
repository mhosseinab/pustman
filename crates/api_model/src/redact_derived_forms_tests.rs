use super::*;

/// Obviously fake, and holding every character the encoders rewrite: `+ / =`, a space and
/// a double quote.
const SECRET: &str = "ab+c/d=e \"x\"";

/// The secret as each encoder a request or an echoing server applies writes it.
const ENCODED_FORMS: [(&str, &str); 6] = [
    ("form-urlencoded", "ab%2Bc%2Fd%3De+%22x%22"),
    ("percent-encoded component", "ab%2Bc%2Fd%3De%20%22x%22"),
    ("percent-encoded path segment", "ab+c%2Fd=e%20%22x%22"),
    ("percent-encoded query", "ab+c/d=e%20%22x%22"),
    ("JSON-escaped", "ab+c/d=e \\\"x\\\""),
    ("JSON-escaped with escaped solidus", "ab+c\\/d=e \\\"x\\\""),
];

#[test]
fn a_secret_is_masked_in_its_percent_form_and_json_encoded_forms() {
    let secrets = SecretValues::from_values([SECRET]);
    for (form, encoded) in ENCODED_FORMS {
        let text = format!("before {encoded} after");
        assert_eq!(
            redact_text(&text, &secrets),
            format!("before {MASK} after"),
            "the {form} form of a secret must be masked"
        );
    }
}

#[test]
fn an_inserted_secret_is_masked_in_its_encoded_forms_too() {
    let mut secrets = SecretValues::new();
    assert!(secrets.insert(SECRET), "the fixture secret is maskable");
    let body = format!("key={}&other=1", ENCODED_FORMS[0].1);
    assert_eq!(redact_text(&body, &secrets), format!("key={MASK}&other=1"));
    assert_eq!(
        secrets.refused_count(),
        0,
        "derived forms are never refused"
    );
}

#[test]
fn a_location_header_carrying_an_encoded_secret_is_masked() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    let headers = vec![(
        "Location".to_string(),
        "https://example.test/next?token=token%2DREDACTED%2Dexample".to_string(),
    )];
    let redacted = redact_headers(&headers, &secrets);
    let value = redacted.first().map(|(_, value)| value.as_str());
    assert_eq!(
        value,
        Some(format!("https://example.test/next?token={MASK}").as_str()),
        "a URL-valued header is redacted as a URL, decoding its components"
    );
}
