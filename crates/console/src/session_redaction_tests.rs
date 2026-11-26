use api_model::{CloseReason, ConnectionDetails, MASK, SecretValues, SessionError, SessionEvent};

use crate::ConsoleLevel;
use crate::session_entry::SessionRecorder;

const TARGET: &str = "wss://echo.example.test/socket";

fn header(name: &str, value: &str) -> (String, String) {
    (name.to_string(), value.to_string())
}

fn connected(request_headers: Vec<(String, String)>) -> SessionEvent {
    SessionEvent::Connected {
        details: ConnectionDetails {
            request_headers,
            response_status: Some(101),
            response_headers: Vec::new(),
            protocol: None,
        },
    }
}

#[test]
fn test_handshake_tokens_echoed_in_error_and_close_reason_are_masked() {
    let subprotocol_token = "subprotocol-REDACTED-example";
    let metadata_token = "metadata-REDACTED-example";
    let mut recorder = SessionRecorder::new("websocket", TARGET, SecretValues::new());
    recorder.record(&connected(vec![
        header("Sec-WebSocket-Protocol", subprotocol_token),
        header("auth-token-bin", metadata_token),
    ]));
    recorder.record(&SessionEvent::Error(SessionError {
        message: format!("invalid token {subprotocol_token} / {metadata_token}"),
    }));
    recorder.record(&SessionEvent::Closed {
        reason: CloseReason::Server {
            code: Some(4401),
            reason: format!("rejected {subprotocol_token} and {metadata_token}"),
        },
    });

    let entry = recorder.entry();
    for secret in [subprotocol_token, metadata_token] {
        assert!(
            !entry.raw_response.contains(secret),
            "{secret} leaked through a server echo"
        );
    }
    assert!(
        entry
            .raw_response
            .contains(&format!("Error: invalid token {MASK}"))
    );
    assert!(entry.raw_response.contains(&format!("rejected {MASK}")));
}

#[test]
fn test_unmaskable_caller_secret_raises_warning_with_note() {
    let recorder = SessionRecorder::new("websocket", TARGET, SecretValues::from_values(["abc"]));

    let entry = recorder.entry();
    assert_eq!(entry.level, ConsoleLevel::Warning);
    assert!(
        entry
            .raw_response
            .contains("1 secret value cannot be masked and appears unmasked.")
    );
}

#[test]
fn test_repeated_short_handshake_token_is_counted_once() {
    let mut recorder = SessionRecorder::new("websocket", TARGET, SecretValues::new());
    recorder.record(&SessionEvent::Connected {
        details: ConnectionDetails {
            request_headers: vec![header("Sec-WebSocket-Protocol", "ab, ab")],
            response_status: Some(101),
            response_headers: vec![header("Sec-WebSocket-Protocol", "ab")],
            protocol: Some("ab".to_string()),
        },
    });

    let entry = recorder.entry();
    assert_eq!(entry.level, ConsoleLevel::Warning);
    assert!(
        entry
            .raw_response
            .contains("1 secret value cannot be masked and appears unmasked."),
        "one distinct token is one value, however often it appears"
    );
}

#[test]
fn test_short_binary_metadata_value_is_masked_whole() {
    let mut recorder = SessionRecorder::new("grpc", TARGET, SecretValues::new());
    recorder.record(&connected(vec![header("auth-bin", "xy")]));

    let entry = recorder.entry();
    assert!(entry.raw_request.contains(&format!("auth-bin: {MASK}")));
    assert!(!entry.raw_request.contains("auth-bin: xy"));
}
