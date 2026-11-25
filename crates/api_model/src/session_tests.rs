use base64::Engine;

use crate::{
    CloseReason, ConnectionDetails, MASK, Message, MessageKind, Payload, ProtocolFields,
    SecretValues, SessionError, SessionEvent, WebSocketOpcode, to_jsonl_line,
};

const SECRET: &str = "token-REDACTED-example";

fn secrets() -> SecretValues {
    SecretValues::from_values([SECRET])
}

fn redacted_line(event: &SessionEvent) -> String {
    to_jsonl_line(&event.redacted(&secrets()))
}

fn received_text(payload: Payload, fields: ProtocolFields) -> SessionEvent {
    SessionEvent::Received(Message {
        at: 1_767_225_600_000,
        kind: MessageKind::Text,
        size: 1024,
        payload,
        fields,
    })
}

fn decoded_payload(line: &str) -> Vec<u8> {
    let value = parse_line(line);
    let payload = &value["message"]["payload"];
    let data = payload["data"].as_str().unwrap_or_default();
    if payload["encoding"] == "base64" {
        match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(bytes) => bytes,
            Err(error) => panic!("payload is not base64 ({error}): {line}"),
        }
    } else {
        data.as_bytes().to_vec()
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

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

#[test]
fn debug_prints_no_query_error_text_or_close_reason_text() {
    let connecting = SessionEvent::Connecting {
        target: format!("wss://host/x?token={SECRET}"),
    };
    let error = SessionEvent::Error(SessionError {
        message: format!("server rejected {SECRET}"),
    });
    let closed = SessionEvent::Closed {
        reason: CloseReason::Server {
            code: Some(4001),
            reason: format!("bad {SECRET}"),
        },
    };

    for event in [&connecting, &error, &closed] {
        let debug = format!("{event:?}");
        assert!(!debug.contains(SECRET), "Debug leaked text: {debug}");
    }
    let connecting_debug = format!("{connecting:?}");
    assert!(
        connecting_debug.contains("wss://host/x"),
        "Debug keeps scheme, host and path: {connecting_debug}"
    );
    assert!(
        format!("{closed:?}").contains("4001"),
        "Debug keeps the close code"
    );
}

#[test]
fn secret_in_text_payload_is_masked_in_export() {
    let event = received_text(
        Payload::Inline(format!("{{\"token\":\"{SECRET}\"}}").into_bytes()),
        ProtocolFields::None,
    );

    let line = redacted_line(&event);
    assert!(!line.contains(SECRET), "the secret leaked: {line}");
    assert_eq!(
        parse_line(&line)["message"]["payload"]["data"],
        format!("{{\"token\":\"{MASK}\"}}")
    );
}

#[test]
fn secret_straddling_spilled_prefix_end_is_cut() {
    let head = "auth ";
    let event = received_text(
        Payload::Spilled {
            path: "/tmp/spill-1".into(),
            len: 4096,
            prefix: format!("{head}{}", &SECRET[..14]).into_bytes(),
        },
        ProtocolFields::None,
    );

    let line = redacted_line(&event);
    assert!(
        !line.contains(&SECRET[..14]),
        "the start of a secret cut by the prefix end leaked: {line}"
    );
    let value = parse_line(&line);
    assert_eq!(value["message"]["payload"]["data"], head);
    assert_eq!(value["message"]["payload"]["spilled"], true);
    assert_eq!(value["message"]["payload"]["len"], 4096);
}

#[test]
fn secret_in_close_reason_and_error_is_masked() {
    let closed = SessionEvent::Closed {
        reason: CloseReason::Server {
            code: Some(4001),
            reason: format!("bad token {SECRET}"),
        },
    };
    let error = SessionEvent::Error(SessionError {
        message: format!("rejected {SECRET}"),
    });

    let closed_line = redacted_line(&closed);
    assert!(!closed_line.contains(SECRET), "leaked: {closed_line}");
    assert_eq!(
        parse_line(&closed_line)["reason"]["reason"],
        format!("bad token {MASK}")
    );
    let error_line = redacted_line(&error);
    assert!(!error_line.contains(SECRET), "leaked: {error_line}");
    assert_eq!(
        parse_line(&error_line)["message"],
        format!("rejected {MASK}")
    );
}

#[test]
fn non_utf8_text_payload_is_still_redacted() {
    let mut bytes = b"before \xff after ".to_vec();
    bytes.extend_from_slice(SECRET.as_bytes());
    bytes.extend_from_slice(b" \xfe end");
    let event = received_text(Payload::Inline(bytes), ProtocolFields::None);

    let line = redacted_line(&event);
    let exported = decoded_payload(&line);
    assert!(
        !contains_bytes(&exported, SECRET.as_bytes()),
        "a secret in invalid UTF-8 text leaked: {line}"
    );
    let mut expected = b"before \xff after ".to_vec();
    expected.extend_from_slice(MASK.as_bytes());
    expected.extend_from_slice(b" \xfe end");
    assert_eq!(exported, expected, "invalid bytes are kept, text is masked");
}

#[test]
fn spilled_prefix_cut_inside_a_multibyte_character_is_redacted() {
    // "é" is 0xC3 0xA9; the prefix ends after its first byte.
    let mut prefix = format!("key {SECRET} caf").into_bytes();
    prefix.push(0xC3);
    let event = received_text(
        Payload::Spilled {
            path: "/tmp/spill-2".into(),
            len: 2048,
            prefix,
        },
        ProtocolFields::None,
    );

    let line = redacted_line(&event);
    assert!(!line.contains(SECRET), "the secret leaked: {line}");
    let value = parse_line(&line);
    assert_eq!(value["message"]["payload"]["encoding"], "utf8");
    assert_eq!(
        value["message"]["payload"]["data"],
        format!("key {MASK} caf"),
        "the partial trailing character is dropped"
    );
}

#[test]
fn secret_in_protocol_fields_is_masked_in_export() {
    let sse = received_text(
        Payload::Inline(b"data".to_vec()),
        ProtocolFields::Sse {
            event: Some(format!("evt-{SECRET}")),
            id: Some(SECRET.to_owned()),
            retry: None,
        },
    );
    let socket_io = received_text(
        Payload::Inline(b"data".to_vec()),
        ProtocolFields::SocketIo {
            event: format!("room:{SECRET}"),
            ack_id: Some(7),
        },
    );

    let sse_line = redacted_line(&sse);
    assert!(!sse_line.contains(SECRET), "leaked: {sse_line}");
    let sse_fields = &parse_line(&sse_line)["message"]["fields"];
    assert_eq!(sse_fields["id"], MASK);
    assert_eq!(sse_fields["event"], format!("evt-{MASK}"));

    let socket_io_line = redacted_line(&socket_io);
    assert!(!socket_io_line.contains(SECRET), "leaked: {socket_io_line}");
    let socket_io_fields = &parse_line(&socket_io_line)["message"]["fields"];
    assert_eq!(socket_io_fields["event"], format!("room:{MASK}"));
    assert_eq!(socket_io_fields["ack_id"], 7);
}
