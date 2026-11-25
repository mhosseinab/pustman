use api_model::{Payload, ProtocolFields, SessionMessageKind};

use super::{EventSourceParser, SseEvent};

fn parse_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
    let mut parser = EventSourceParser::new();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(parser.feed(chunk));
    }
    events.extend(parser.finish());
    events
}

fn event(kind: Option<&str>, data: &str, id: Option<&str>, retry: Option<u64>) -> SseEvent {
    SseEvent {
        event: kind.map(str::to_owned),
        data: data.to_owned(),
        id: id.map(str::to_owned),
        retry,
    }
}

#[test]
fn multi_line_data_joined_with_newlines() {
    let cases: &[(&str, &[u8], Vec<SseEvent>)] = &[
        (
            "LF line ends",
            b"data: first\ndata: second\n\n",
            vec![event(None, "first\nsecond", None, None)],
        ),
        (
            "CRLF line ends",
            b"data: first\r\ndata: second\r\n\r\n",
            vec![event(None, "first\nsecond", None, None)],
        ),
        (
            "CR line ends",
            b"data: first\rdata: second\r\r",
            vec![event(None, "first\nsecond", None, None)],
        ),
        (
            "mixed line ends",
            b"data: a\rdata: b\ndata: c\r\n\n",
            vec![event(None, "a\nb\nc", None, None)],
        ),
        (
            "empty data lines keep their newlines",
            b"data\ndata:\ndata: x\n\n",
            vec![event(None, "\n\nx", None, None)],
        ),
        (
            "only one leading space is stripped",
            b"data:  two spaces\ndata:none\n\n",
            vec![event(None, " two spaces\nnone", None, None)],
        ),
        (
            "event, id and data together; id persists to later events",
            b"event: update\nid: 7\ndata: {\"a\":1}\n\ndata: next\n\n",
            vec![
                event(Some("update"), "{\"a\":1}", Some("7"), None),
                event(None, "next", Some("7"), None),
            ],
        ),
        (
            "an empty id resets the last event id",
            b"id: 1\ndata: a\n\nid\ndata: b\n\n",
            vec![
                event(None, "a", Some("1"), None),
                event(None, "b", None, None),
            ],
        ),
        (
            "an id containing NUL is ignored",
            b"id: 1\ndata: a\n\nid: 2\0x\ndata: b\n\n",
            vec![
                event(None, "a", Some("1"), None),
                event(None, "b", Some("1"), None),
            ],
        ),
        (
            "a block without data is not dispatched",
            b"event: ping\n\ndata: real\n\n",
            vec![event(None, "real", None, None)],
        ),
        (
            "unknown fields are ignored",
            b"foo: bar\ndata: x\n\n",
            vec![event(None, "x", None, None)],
        ),
        (
            "an unterminated event at stream end is dropped",
            b"data: kept\n\ndata: dropped\n",
            vec![event(None, "kept", None, None)],
        ),
        (
            "a partial line at stream end is dropped",
            b"data: kept\n\ndata: partial",
            vec![event(None, "kept", None, None)],
        ),
        (
            "invalid UTF-8 becomes the replacement character",
            b"data: a\xffb\n\n",
            vec![event(None, "a\u{fffd}b", None, None)],
        ),
    ];
    for (name, input, expected) in cases {
        assert_eq!(&parse_all(&[input]), expected, "{name}");
    }

    let message = event(Some("update"), "a\nb", Some("7"), Some(1500)).into_message(42);
    assert_eq!(message.at, 42);
    assert_eq!(message.kind, SessionMessageKind::Event);
    assert_eq!(message.payload, Payload::Inline(b"a\nb".to_vec()));
    assert_eq!(message.size, 3);
    assert_eq!(
        message.fields,
        ProtocolFields::Sse {
            event: Some("update".to_owned()),
            id: Some("7".to_owned()),
            retry: Some(1500),
        }
    );
}

#[test]
fn event_split_across_every_byte_boundary() {
    // CR, CRLF and LF terminators, a multi-byte UTF-8 sequence, a BOM and a comment: every
    // split point must give the same events as one chunk, including a CRLF split between
    // its CR and its LF and a code point split between its bytes.
    let stream: &[u8] = "\u{feff}: hi\r\nevent: grüße\rid: ✓1\r\ndata: 日本\r\ndata: x\n\nretry: 250\ndata: two\r\r"
        .as_bytes();
    let expected = vec![
        event(Some("grüße"), "日本\nx", Some("✓1"), None),
        event(None, "two", Some("✓1"), Some(250)),
    ];
    assert_eq!(parse_all(&[stream]), expected, "one chunk");

    for split in 0..=stream.len() {
        let (head, tail) = stream.split_at(split);
        assert_eq!(parse_all(&[head, tail]), expected, "split at {split}");
    }

    let bytes: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(parse_all(&bytes), expected, "one byte at a time");
}

#[test]
fn comments_and_bom_ignored() {
    let cases: &[(&str, &[u8], Vec<SseEvent>)] = &[
        (
            "comment lines",
            b": keep-alive\ndata: x\n:\n: another\n\n",
            vec![event(None, "x", None, None)],
        ),
        (
            "a comment-only block dispatches nothing",
            b": ping\n\n: ping\n\n",
            vec![],
        ),
        (
            "a leading BOM",
            b"\xef\xbb\xbfdata: x\n\n",
            vec![event(None, "x", None, None)],
        ),
        (
            "a BOM before a comment",
            b"\xef\xbb\xbf: c\ndata: x\n\n",
            vec![event(None, "x", None, None)],
        ),
        (
            "only the first BOM is stripped",
            b"\xef\xbb\xbf\xef\xbb\xbfdata: x\n\n",
            vec![],
        ),
        (
            "a BOM later in the stream is data",
            b"data: \xef\xbb\xbfx\n\n",
            vec![event(None, "\u{feff}x", None, None)],
        ),
    ];
    for (name, input, expected) in cases {
        assert_eq!(&parse_all(&[input]), expected, "{name}");
    }

    let mut parser = EventSourceParser::new();
    assert_eq!(parser.feed(b"\xef"), vec![]);
    assert_eq!(parser.feed(b"\xbb"), vec![]);
    assert_eq!(
        parser.feed(b"\xbfdata: split bom\n\n"),
        vec![event(None, "split bom", None, None)],
        "a BOM split across chunks is still stripped"
    );

    let mut parser = EventSourceParser::new();
    assert_eq!(
        parser.feed(b"\xefdata: x\n\n"),
        vec![],
        "a lone first BOM byte makes the field name invalid, not a stripped BOM"
    );
}

#[test]
fn retry_accepts_digits_only() {
    let cases: &[(&str, &[u8], Option<u64>)] = &[
        ("digits", b"retry: 3000\ndata: x\n\n", Some(3000)),
        ("zero", b"retry: 0\ndata: x\n\n", Some(0)),
        ("negative", b"retry: -1\ndata: x\n\n", None),
        ("decimal", b"retry: 1.5\ndata: x\n\n", None),
        ("trailing text", b"retry: 10ms\ndata: x\n\n", None),
        ("trailing space", b"retry: 10 \ndata: x\n\n", None),
        ("two leading spaces", b"retry:  10\ndata: x\n\n", None),
        ("plus sign", b"retry: +10\ndata: x\n\n", None),
        ("empty", b"retry:\ndata: x\n\n", None),
        ("no colon", b"retry\ndata: x\n\n", None),
        (
            "non-ASCII digits",
            "retry: ١٢\ndata: x\n\n".as_bytes(),
            None,
        ),
        (
            "too large for u64",
            b"retry: 99999999999999999999999\ndata: x\n\n",
            None,
        ),
    ];
    for (name, input, retry) in cases {
        assert_eq!(
            parse_all(&[input]),
            vec![event(None, "x", None, *retry)],
            "{name}"
        );
    }

    let mut parser = EventSourceParser::new();
    let events = parser.feed(b"retry: 500\n\ndata: x\n\n");
    assert_eq!(
        events,
        vec![event(None, "x", None, None)],
        "retry belongs to the block it appears in"
    );
    assert_eq!(
        parser.reconnection_time(),
        Some(500),
        "a retry-only block still sets the reconnection time"
    );

    let mut parser = EventSourceParser::new();
    parser.feed(b"retry: 500\nretry: abc\ndata: x\n\n");
    assert_eq!(
        parser.reconnection_time(),
        Some(500),
        "an invalid retry keeps the previous reconnection time"
    );
}
