use api_model::{
    MASK, Message, Payload, ProtocolFields, SecretValues, SessionEvent, SessionMessageKind,
    to_jsonl_line,
};
use serde_json::Value;

use super::*;

/// A fake session secret: obviously not a real credential (tests.md).
const FAKE_SECRET: &str = "token-REDACTED-example";

/// The pushes spill through `http_engine::SpillWriter`, whose writes are tokio futures, so
/// each test drives the store on a tokio runtime of its own (the `large_body_tests` shape).
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test's tokio runtime must build")
}

fn message(kind: SessionMessageKind, payload: Vec<u8>) -> Message {
    Message {
        at: 1_700_000_000_000,
        kind,
        size: payload.len() as u64,
        payload: Payload::Inline(payload),
        fields: ProtocolFields::None,
    }
}

fn received_text(text: &str) -> SessionEvent {
    SessionEvent::Received(message(SessionMessageKind::Text, text.as_bytes().to_vec()))
}

fn sent_text(text: &str) -> SessionEvent {
    SessionEvent::Sent(message(SessionMessageKind::Text, text.as_bytes().to_vec()))
}

/// `len` bytes of fake text, so a spilled payload is recognisable byte for byte.
fn big_text(len: usize) -> Vec<u8> {
    b"abcdefghij".iter().copied().cycle().take(len).collect()
}

fn spilled_path(event: &SessionEvent) -> std::path::PathBuf {
    match event {
        SessionEvent::Sent(message) | SessionEvent::Received(message) => match &message.payload {
            Payload::Spilled { path, .. } => path.clone(),
            Payload::Inline(_) => panic!("the payload must have been spilled"),
        },
        _ => panic!("the event must be a message"),
    }
}

#[test]
fn hundred_thousand_events_keep_inline_bytes_bounded() {
    // 100,000 small messages hold 20 MB; eight 1.5 MB messages would add 12 MB more if
    // they stayed in memory. Spilled, each keeps only its 4 KB preview.
    const CAP: usize = 24 * 1024 * 1024;
    let runtime = runtime();
    let mut store = SessionStore::new(SecretValues::new());
    let small = "x".repeat(200);
    runtime.block_on(async {
        for index in 0..100_000 {
            let event = if index % 12_500 == 0 {
                SessionEvent::Received(message(SessionMessageKind::Text, big_text(1536 * 1024)))
            } else {
                received_text(&small)
            };
            store.push(event).await.expect("the push must succeed");
        }
    });
    assert_eq!(store.len(), 100_000);
    assert!(
        store.inline_bytes() < CAP,
        "inline bytes {} must stay under the {CAP} byte cap",
        store.inline_bytes()
    );
}

#[test]
fn payload_over_one_megabyte_is_spilled_with_prefix() {
    let runtime = runtime();
    let mut store = SessionStore::new(SecretValues::new());
    let payload = big_text(SPILL_THRESHOLD + 1);
    let at_threshold = big_text(SPILL_THRESHOLD);
    runtime.block_on(async {
        store
            .push(SessionEvent::Received(message(
                SessionMessageKind::Text,
                payload.clone(),
            )))
            .await
            .expect("the push must succeed");
        store
            .push(SessionEvent::Received(message(
                SessionMessageKind::Text,
                at_threshold.clone(),
            )))
            .await
            .expect("the push must succeed");
    });

    let Some(SessionEvent::Received(spilled)) = store.events().first() else {
        panic!("the first event must be the received message");
    };
    let Payload::Spilled { path, len, prefix } = &spilled.payload else {
        panic!("a payload over 1 MB must be spilled");
    };
    assert_eq!(*len, payload.len() as u64);
    assert_eq!(spilled.size, payload.len() as u64);
    assert_eq!(prefix.as_slice(), &payload[..PREVIEW_LEN]);
    let tail = std::fs::read(path).expect("the spill file must exist");
    assert_eq!(tail.as_slice(), &payload[PREVIEW_LEN..]);

    let Some(SessionEvent::Received(kept)) = store.events().get(1) else {
        panic!("the second event must be the received message");
    };
    assert_eq!(kept.payload, Payload::Inline(at_threshold.clone()));
    assert_eq!(store.inline_bytes(), PREVIEW_LEN + at_threshold.len());
}

#[test]
fn filter_by_direction_and_text() {
    let runtime = runtime();
    let mut store = SessionStore::new(SecretValues::new());
    runtime.block_on(async {
        for event in [
            SessionEvent::Connecting {
                target: "wss://example.test/alpha".to_owned(),
            },
            sent_text("hello alpha"),
            received_text("alpha beta"),
            received_text("gamma"),
            SessionEvent::Received(message(SessionMessageKind::Json, b"{\"alpha\":1}".to_vec())),
        ] {
            store.push(event).await.expect("the push must succeed");
        }
    });
    assert_eq!(
        store.filtered(),
        &[0, 1, 2, 3, 4],
        "no filter shows every row"
    );

    store
        .set_filter(SessionFilter {
            direction: Some(SessionDirection::Received),
            kind: None,
            text: "ALPHA".to_owned(),
        })
        .expect("the filter must compile");
    assert_eq!(store.filtered(), &[2, 4]);

    store
        .set_filter(SessionFilter {
            direction: Some(SessionDirection::Received),
            kind: Some(SessionMessageKind::Text),
            text: "alpha".to_owned(),
        })
        .expect("the filter must compile");
    assert_eq!(store.filtered(), &[2]);

    // A row pushed after the filter is set joins the index without a rebuild.
    runtime.block_on(async {
        store
            .push(received_text("more Alpha"))
            .await
            .expect("the push must succeed");
        store
            .push(sent_text("alpha again"))
            .await
            .expect("the push must succeed");
    });
    assert_eq!(store.filtered(), &[2, 5]);

    store
        .set_filter(SessionFilter {
            direction: Some(SessionDirection::Sent),
            kind: None,
            text: String::new(),
        })
        .expect("the filter must compile");
    assert_eq!(store.filtered(), &[1, 6]);
}

#[test]
fn clear_deletes_spill_files() {
    let runtime = runtime();
    let mut store = SessionStore::new(SecretValues::new());
    let mut dropped = SessionStore::new(SecretValues::new());
    runtime.block_on(async {
        for target in [&mut store, &mut dropped] {
            target
                .push(SessionEvent::Received(message(
                    SessionMessageKind::Binary,
                    big_text(SPILL_THRESHOLD + 10),
                )))
                .await
                .expect("the push must succeed");
        }
    });
    let cleared_path = store.events().first().map(spilled_path).expect("one event");
    let dropped_path = dropped
        .events()
        .first()
        .map(spilled_path)
        .expect("one event");
    assert!(cleared_path.exists() && dropped_path.exists());

    store.clear();
    assert!(!cleared_path.exists(), "clear must delete the spill file");
    assert!(store.is_empty());
    assert!(store.filtered().is_empty());
    assert_eq!(store.inline_bytes(), 0);

    drop(dropped);
    assert!(
        !dropped_path.exists(),
        "dropping the store must delete its spill files"
    );
}

#[test]
fn export_jsonl_streams_spilled_payloads() {
    let runtime = runtime();
    let mut store = SessionStore::new(SecretValues::from_values([FAKE_SECRET]));

    // The secret sits past the preview, so only the spilled tail holds it.
    let mut text = big_text(SPILL_THRESHOLD + 5_000);
    text.extend_from_slice(FAKE_SECRET.as_bytes());
    text.extend_from_slice(b" end");
    // A binary payload whose length is not a multiple of three and whose bytes are not
    // UTF-8: streamed base64 must carry across read chunks exactly.
    let binary: Vec<u8> = (0..SPILL_THRESHOLD + 7)
        .map(|index| u8::try_from(index % 251).unwrap_or(0) | 0x80)
        .collect();
    let events = [
        SessionEvent::Connecting {
            target: "wss://example.test/stream".to_owned(),
        },
        sent_text("small"),
        SessionEvent::Received(message(SessionMessageKind::Text, text.clone())),
        SessionEvent::Received(message(SessionMessageKind::Binary, binary.clone())),
    ];
    runtime.block_on(async {
        for event in events.clone() {
            store.push(event).await.expect("the push must succeed");
        }
    });

    let mut exported = Vec::new();
    store
        .export_jsonl(&mut exported)
        .expect("the export must succeed");
    let lines: Vec<&[u8]> = exported
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), events.len());

    // Every line is exactly what the shared JSONL format writes for the whole (redacted)
    // message held inline: the spilled bytes are all there, and nothing is marked partial.
    let redacted_text = String::from_utf8(text.clone())
        .expect("the fake text is UTF-8")
        .replace(FAKE_SECRET, MASK)
        .into_bytes();
    let expected = [
        to_jsonl_line(&events[0]),
        to_jsonl_line(&events[1]),
        to_jsonl_line(&SessionEvent::Received(Message {
            payload: Payload::Inline(redacted_text),
            ..message(SessionMessageKind::Text, text.clone())
        })),
        to_jsonl_line(&events[3]),
    ];
    for (line, expected) in lines.iter().zip(expected.iter()) {
        let line: Value = serde_json::from_slice(line).expect("each line is JSON");
        let expected: Value = serde_json::from_slice(expected.as_bytes()).expect("JSON");
        assert!(
            line == expected,
            "an exported line differs from the inline form"
        );
    }
    let exported_text = String::from_utf8_lossy(&exported);
    assert!(
        !exported_text.contains(FAKE_SECRET),
        "the secret must be masked"
    );
}
