use base64::Engine;

use crate::{
    CloseReason, ConnectionDetails, MASK, Message, MessageKind, Payload, ProtocolFields,
    SessionError, SessionEvent, WebSocketOpcode, to_jsonl_line,
};

fn text_message(text: &str) -> Message {
    Message {
        at: 1_767_225_600_123,
        kind: MessageKind::Text,
        size: text.len() as u64,
        payload: Payload::Inline(text.as_bytes().to_vec()),
        fields: ProtocolFields::WebSocket {
            opcode: WebSocketOpcode::Text,
        },
    }
}

fn parse_line(line: &str) -> serde_json::Value {
    match serde_json::from_slice::<serde_json::Value>(line.as_bytes()) {
        Ok(value) => value,
        Err(error) => panic!("line is not one JSON value ({error}): {line}"),
    }
}

#[test]
fn each_event_serialises_to_one_json_line() {
    let events = [
        SessionEvent::Connecting {
            target: "wss://echo.example.test/socket".to_owned(),
        },
        SessionEvent::Connected {
            details: ConnectionDetails {
                request_headers: vec![("Sec-WebSocket-Version".to_owned(), "13".to_owned())],
                response_status: Some(101),
                response_headers: vec![("Upgrade".to_owned(), "websocket".to_owned())],
                protocol: Some("chat".to_owned()),
            },
        },
        SessionEvent::Sent(text_message("first line\nsecond line\r\n")),
        SessionEvent::Received(Message {
            at: 1_767_225_600_456,
            kind: MessageKind::Event,
            size: 7,
            payload: Payload::Inline(b"{\"a\":1}".to_vec()),
            fields: ProtocolFields::Sse {
                event: Some("update".to_owned()),
                id: Some("42".to_owned()),
                retry: Some(3000),
            },
        }),
        SessionEvent::Error(SessionError {
            message: "connection reset\nby peer".to_owned(),
        }),
        SessionEvent::Closed {
            reason: CloseReason::Server {
                code: Some(1000),
                reason: "bye".to_owned(),
            },
        },
    ];
    let expected_tags = [
        "connecting",
        "connected",
        "sent",
        "received",
        "error",
        "closed",
    ];

    for (event, expected_tag) in events.iter().zip(expected_tags) {
        let line = to_jsonl_line(event);
        assert!(
            !line.contains('\n') && !line.contains('\r'),
            "a JSONL line holds no raw line break: {line:?}"
        );
        let value = parse_line(&line);
        assert!(value.is_object(), "each line is one JSON object: {line}");
        assert_eq!(value["event"], expected_tag, "event tag of {line}");
    }

    let sent = parse_line(&to_jsonl_line(&events[2]));
    assert_eq!(sent["message"]["payload"]["encoding"], "utf8");
    assert_eq!(
        sent["message"]["payload"]["data"],
        "first line\nsecond line\r\n"
    );
    assert_eq!(sent["message"]["at"], 1_767_225_600_123_u64);
    assert_eq!(sent["message"]["fields"]["protocol"], "websocket");
    assert_eq!(sent["message"]["fields"]["opcode"], "text");

    let received = parse_line(&to_jsonl_line(&events[3]));
    assert_eq!(received["message"]["fields"]["protocol"], "sse");
    assert_eq!(received["message"]["fields"]["retry"], 3000);
}

#[test]
fn binary_payload_exports_as_base64() {
    let bytes = vec![0x00, 0x9f, 0x92, 0x96, 0xff, b'\n'];
    let event = SessionEvent::Received(Message {
        at: 1_767_225_600_789,
        kind: MessageKind::Binary,
        size: bytes.len() as u64,
        payload: Payload::Inline(bytes.clone()),
        fields: ProtocolFields::Grpc { message_index: 3 },
    });

    let value = parse_line(&to_jsonl_line(&event));
    let payload = &value["message"]["payload"];
    assert_eq!(payload["encoding"], "base64");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    assert_eq!(payload["data"], encoded.as_str());
    let decoded = payload["data"]
        .as_str()
        .map(|data| base64::engine::general_purpose::STANDARD.decode(data));
    assert!(matches!(decoded, Some(Ok(ref round_trip)) if *round_trip == bytes));
    assert_eq!(value["message"]["kind"], "binary");
    assert_eq!(value["message"]["fields"]["message_index"], 3);
}

#[test]
fn connection_details_redact_authorization() {
    let token = "token-REDACTED-example";
    let details = ConnectionDetails {
        request_headers: vec![
            ("Authorization".to_owned(), format!("Bearer {token}")),
            ("Accept".to_owned(), "*/*".to_owned()),
        ],
        response_status: Some(101),
        response_headers: vec![("Set-Cookie".to_owned(), format!("session={token}"))],
        protocol: None,
    };
    let event = SessionEvent::Connected {
        details: details.clone(),
    };

    let line = to_jsonl_line(&event);
    assert!(!line.contains(token), "the credential leaked: {line}");
    let value = parse_line(&line);
    let request_headers = &value["details"]["request_headers"];
    assert_eq!(request_headers[0][0], "Authorization");
    assert_eq!(request_headers[0][1], MASK);
    assert_eq!(request_headers[1][1], "*/*");
    assert_eq!(value["details"]["response_headers"][0][1], MASK);
    assert_eq!(value["details"]["response_status"], 101);

    assert!(
        !format!("{details:?}").contains(token),
        "Debug output must not carry the credential either"
    );
}

#[test]
fn connecting_debug_never_prints_url_credentials() {
    let event = SessionEvent::Connecting {
        target: "wss://user:hunter2-example@host/x".to_owned(),
    };

    let debug = format!("{event:?}");
    assert!(
        !debug.contains("hunter2-example"),
        "Debug leaked the URL password: {debug}"
    );
    assert!(
        debug.contains("host/x"),
        "Debug still names the target: {debug}"
    );
}
