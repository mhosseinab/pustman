use std::time::Duration;

use api_model::{Method, SecretValues, redact_text};

use crate::{ConsoleEntry, ConsoleLevel, export_text};

/// Obviously fake fixture values (`.agents/rules/tests.md`): one registered secret and one
/// bearer token deliberately kept out of `secrets`, so only the credential-header rule can
/// hide it.
const REGISTERED_SECRET: &str = "fake-registered-secret-EXAMPLE";
const UNREGISTERED_TOKEN: &str = "fake-bearer-token-EXAMPLE";

#[test]
fn export_text_contains_no_secret() {
    // The export must hide a registered secret wherever the entry carries it — summary
    // URL, request line, request body — and must hide a credential header's value even
    // though that value is not in `secrets` (REQ-CON-05, defense in depth).
    let secrets = SecretValues::from_values([REGISTERED_SECRET]);
    assert_eq!(
        secrets.refused_count(),
        0,
        "the fixture secret must be maskable, or the test proves nothing"
    );
    let entry = ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_string(),
        method: Some(Method::Get),
        url: format!("https://api.example.test/v1/users?token={REGISTERED_SECRET}"),
        status: Some(200),
        time: Some(Duration::from_millis(12)),
        size: Some(34),
        raw_request: format!(
            "GET https://api.example.test/v1/users?token={REGISTERED_SECRET} HTTP/1.1\r\n\
             Host: api.example.test\r\n\
             Authorization: Bearer {UNREGISTERED_TOKEN}\r\n\
             \r\n\
             {{\"password\": \"{REGISTERED_SECRET}\"}}\r\n"
        ),
        raw_response: format!(
            "HTTP/1.1 200 OK\r\n\
             Set-Cookie: session={UNREGISTERED_TOKEN}; HttpOnly\r\n\
             \r\n\
             {{\"echo\": \"{REGISTERED_SECRET}\"}}\n"
        ),
    };

    let exported = export_text(std::slice::from_ref(&entry), &secrets);

    assert!(
        !exported.contains(REGISTERED_SECRET),
        "a registered secret must not survive the export: not in the summary URL, the \
         request line, the request body nor the response body"
    );
    assert!(
        !exported.contains(UNREGISTERED_TOKEN),
        "a credential header's value must not survive the export even though it is not in \
         `secrets`"
    );
    let mask = redact_text(REGISTERED_SECRET, &secrets);
    assert!(
        !mask.contains(REGISTERED_SECRET),
        "redaction must actually mask the fixture secret"
    );
    assert!(
        exported.contains(&mask),
        "the export must show the mask where the hidden values were"
    );
}
