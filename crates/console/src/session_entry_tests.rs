use api_model::{
    CloseReason, ConnectionDetails, MASK, Message, Payload, ProtocolFields, SecretValues,
    SessionError, SessionEvent, SessionMessageKind, WebSocketOpcode,
};
use gpui::TestAppContext;

use crate::session_entry::SessionRecorder;
use crate::{ConsoleLevel, ConsoleStore};

/// A payload no Console text may ever contain: only counts and sizes are copied.
const PAYLOAD: &str = "payload-text-never-copied";

fn message(size: u64) -> Message {
    Message {
        at: 1_700_000_000_000,
        kind: SessionMessageKind::Text,
        payload: Payload::Inline(PAYLOAD.as_bytes().to_vec()),
        size,
        fields: ProtocolFields::WebSocket {
            opcode: WebSocketOpcode::Text,
        },
    }
}

fn header(name: &str, value: &str) -> (String, String) {
    (name.to_string(), value.to_string())
}

fn connected(status: u16, response_headers: Vec<(String, String)>) -> SessionEvent {
    SessionEvent::Connected {
        details: ConnectionDetails {
            request_headers: vec![header("Upgrade", "websocket")],
            response_status: Some(status),
            response_headers,
            protocol: None,
        },
    }
}

#[gpui::test]
fn session_records_one_entry_with_counts(cx: &mut TestAppContext) {
    let target = "wss://echo.example.test/socket";
    let mut recorder = SessionRecorder::new("websocket", target, SecretValues::new());
    for event in [
        SessionEvent::Connecting {
            target: target.to_string(),
        },
        connected(101, vec![header("Upgrade", "websocket")]),
        SessionEvent::Sent(message(5)),
        SessionEvent::Received(message(1)),
        SessionEvent::Sent(message(7)),
        SessionEvent::Received(message(2)),
        SessionEvent::Received(message(3)),
        SessionEvent::Closed {
            reason: CloseReason::Server {
                code: Some(1000),
                reason: "bye".to_string(),
            },
        },
    ] {
        recorder.record(&event);
    }

    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()));
        crate::init(cx);
        recorder.push(cx);

        let store = cx.global::<ConsoleStore>();
        assert_eq!(store.len(), 1, "a whole session is one Console entry");
        let Some(entry) = store.entries().front() else {
            panic!("the session's entry is in the store");
        };
        assert_eq!(entry.protocol, "websocket");
        assert_eq!(entry.method, None);
        assert_eq!(entry.url, target);
        assert_eq!(entry.level, ConsoleLevel::Info);
        assert_eq!(entry.status, Some(101));
        assert_eq!(entry.size, Some(6), "the size is the bytes received");
        assert!(entry.raw_response.contains("Sent: 2 messages, 12 bytes"));
        assert!(entry.raw_response.contains("Received: 3 messages, 6 bytes"));
        assert!(
            entry
                .raw_response
                .contains("Closed by the server (code 1000): bye")
        );
        assert!(
            !entry.raw_request.contains(PAYLOAD) && !entry.raw_response.contains(PAYLOAD),
            "payloads are never copied into the Console"
        );
        assert_eq!(entry.notice(), None, "a session entry is not a notice");
    });
}

#[gpui::test]
fn failed_upgrade_records_status_and_body_size(cx: &mut TestAppContext) {
    let target = "wss://echo.example.test/socket";
    let mut recorder = SessionRecorder::new("websocket", target, SecretValues::new());
    recorder.record(&connected(
        401,
        vec![
            header("Content-Type", "text/plain"),
            header("Content-Length", "42"),
        ],
    ));
    recorder.record(&SessionEvent::Error(SessionError {
        message: "The server refused the upgrade: 401 Unauthorized".to_string(),
    }));

    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()));
        crate::init(cx);
        recorder.push(cx);

        let store = cx.global::<ConsoleStore>();
        assert_eq!(store.len(), 1);
        let Some(entry) = store.entries().front() else {
            panic!("the failed session's entry is in the store");
        };
        assert_eq!(entry.level, ConsoleLevel::Error);
        assert_eq!(entry.status, Some(401));
        assert_eq!(
            entry.size,
            Some(42),
            "a refused upgrade's size is its body's"
        );
        assert!(entry.raw_response.contains("Status: 401"));
        assert!(entry.raw_response.contains("Body: 42 bytes"));
        assert!(
            entry
                .raw_response
                .contains("Error: The server refused the upgrade: 401 Unauthorized")
        );
    });
}

#[test]
fn handshake_authorization_is_redacted() {
    let query_secret = "query-REDACTED-example";
    let echoed_secret = "echo-REDACTED-example";
    let bearer = "Bearer token-REDACTED-example";
    let subprotocol_token = "subprotocol-REDACTED-example";
    let metadata_token = "metadata-REDACTED-example";
    let secrets = SecretValues::from_values([query_secret, echoed_secret]);
    let target =
        format!("wss://user:pw-REDACTED-example@echo.example.test/socket?key={query_secret}");
    let mut recorder = SessionRecorder::new("websocket", &target, secrets);

    recorder.record(&SessionEvent::Connected {
        details: ConnectionDetails {
            request_headers: vec![
                header("Authorization", bearer),
                header(
                    "Sec-WebSocket-Protocol",
                    &format!("graphql-ws, {subprotocol_token}"),
                ),
                header("auth-token-bin", metadata_token),
                header("X-Echo", echoed_secret),
            ],
            response_status: Some(101),
            response_headers: vec![
                header("Sec-WebSocket-Protocol", subprotocol_token),
                header("Set-Cookie", "session=cookie-REDACTED-example"),
            ],
            protocol: Some(subprotocol_token.to_string()),
        },
    });
    let long_reason = format!("{echoed_secret} {}", "x".repeat(10_000));
    recorder.record(&SessionEvent::Error(SessionError {
        message: format!("server said {echoed_secret}"),
    }));
    recorder.record(&SessionEvent::Closed {
        reason: CloseReason::Server {
            code: Some(4000),
            reason: long_reason,
        },
    });

    let entry = recorder.entry();
    let texts = [&entry.url, &entry.raw_request, &entry.raw_response];
    for secret in [
        query_secret,
        echoed_secret,
        "token-REDACTED-example",
        subprotocol_token,
        metadata_token,
        "pw-REDACTED-example",
        "cookie-REDACTED-example",
    ] {
        for text in texts {
            assert!(!text.contains(secret), "{secret} leaked into the Console");
        }
    }
    assert!(
        entry
            .raw_request
            .contains(&format!("Authorization: {MASK}"))
    );
    assert!(entry.raw_request.contains("Sec-WebSocket-Protocol"));
    assert!(
        entry.raw_response.len() < 2_000,
        "server-sent close reasons are capped, got {} bytes",
        entry.raw_response.len()
    );
}
