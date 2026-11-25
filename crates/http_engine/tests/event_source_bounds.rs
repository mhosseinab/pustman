//! The SSE parser reads untrusted network bytes: what it buffers and what each event holds
//! must stay bounded whatever the server sends (REQ-SSE-01).

use std::sync::Arc;

use api_model::ProtocolFields;
use http_engine::event_source::{
    EventSourceParser, MAX_EVENT_BYTES, MAX_FIELD_BYTES, MAX_ID_BYTES, SseEvent,
};

fn data_of(events: &[SseEvent]) -> Vec<&str> {
    events.iter().map(|event| event.data.as_str()).collect()
}

#[test]
fn an_unterminated_line_past_the_bound_is_dropped_and_parsing_resumes() {
    let mut parser = EventSourceParser::new();
    let mut huge = b"data: ".to_vec();
    huge.resize(MAX_EVENT_BYTES + 10, b'a');

    assert_eq!(parser.feed(&huge), vec![]);
    assert_eq!(data_of(&parser.feed(b"\n\ndata: ok\n\n")), vec!["ok"]);
    assert_eq!(parser.oversized_events_dropped(), 1);
}

#[test]
fn data_lines_past_the_bound_drop_the_whole_event() {
    let mut parser = EventSourceParser::new();
    let line = {
        let mut line = b"data: ".to_vec();
        line.resize(1024 * 1024, b'b');
        line.push(b'\n');
        line
    };
    let lines_past_the_bound = MAX_EVENT_BYTES / (1024 * 1024) + 4;
    for _ in 0..lines_past_the_bound {
        assert_eq!(parser.feed(&line), vec![]);
    }

    assert_eq!(data_of(&parser.feed(b"\ndata: after\n\n")), vec!["after"]);
    assert_eq!(parser.oversized_events_dropped(), 1);
}

#[test]
fn a_non_blank_line_while_discarding_does_not_end_the_discard() {
    let mut parser = EventSourceParser::new();
    let mut huge = b"data: ".to_vec();
    huge.resize(MAX_EVENT_BYTES + 10, b'a');
    huge.push(b'\n');

    assert_eq!(parser.feed(&huge), vec![]);
    // Still the dropped event: its later data lines must not surface as an event.
    assert_eq!(parser.feed(b"data: tail of the dropped event\n"), vec![]);
    assert_eq!(parser.feed(b"\n"), vec![]);
    assert_eq!(data_of(&parser.feed(b"data: next\n\n")), vec!["next"]);
    assert_eq!(parser.oversized_events_dropped(), 1);
}

#[test]
fn the_event_type_counts_toward_the_event_bound() {
    let mut parser = EventSourceParser::new();
    let mut stream = b"event: ".to_vec();
    stream.resize(stream.len() + 1000, b'e');
    stream.extend_from_slice(b"\ndata: ");
    // The data line alone fits the bound; with the event type it does not.
    stream.resize(stream.len() + MAX_EVENT_BYTES - 500, b'd');
    stream.extend_from_slice(b"\n\ndata: ok\n\n");

    assert_eq!(data_of(&parser.feed(&stream)), vec!["ok"]);
    assert_eq!(parser.oversized_events_dropped(), 1);
}

#[test]
fn a_huge_id_is_ignored_and_counted_so_later_events_keep_the_previous_id() {
    let mut parser = EventSourceParser::new();
    assert_eq!(parser.feed(b"id: kept\n\n"), vec![]);

    let mut huge_id = b"id: ".to_vec();
    huge_id.resize(huge_id.len() + MAX_FIELD_BYTES + 1, b'i');
    huge_id.push(b'\n');
    assert_eq!(parser.feed(&huge_id), vec![]);
    assert_eq!(parser.oversized_fields_ignored(), 1);

    let events = parser.feed(&b"data\n\n".repeat(10_000));
    assert_eq!(events.len(), 10_000);
    for event in &events {
        assert_eq!(event.id.as_deref(), Some("kept"));
    }
    assert_eq!(parser.last_event_id(), "kept");
}

fn id_line(len: usize) -> Vec<u8> {
    let mut line = b"id: ".to_vec();
    line.resize(line.len() + len, b'i');
    line.extend_from_slice(b"\n\n");
    line
}

#[test]
fn an_id_one_byte_over_the_id_cap_is_ignored_and_counted() {
    let mut parser = EventSourceParser::new();
    parser.feed(b"id: kept\n\n");
    parser.feed(&id_line(MAX_ID_BYTES + 1));

    assert_eq!(parser.oversized_fields_ignored(), 1);
    assert_eq!(parser.last_event_id(), "kept");
}

#[test]
fn an_id_exactly_at_the_id_cap_is_kept() {
    let mut parser = EventSourceParser::new();
    parser.feed(&id_line(MAX_ID_BYTES));

    assert_eq!(parser.oversized_fields_ignored(), 0);
    assert_eq!(parser.last_event_id().len(), MAX_ID_BYTES);
}

#[test]
fn a_logged_message_copies_at_most_the_id_cap() {
    let mut parser = EventSourceParser::new();
    parser.feed(&id_line(MAX_ID_BYTES));
    parser.feed(&id_line(MAX_ID_BYTES + 1));
    let events = parser.feed(b"data: x\n\n");

    let message = events.into_iter().next().map(|event| event.into_message(0));
    match message.map(|message| message.fields) {
        Some(ProtocolFields::Sse { id: Some(id), .. }) => assert!(id.len() <= MAX_ID_BYTES),
        other => panic!("expected an SSE message with an id, got {other:?}"),
    }
}

#[test]
fn an_id_at_the_cap_is_shared_between_events_not_copied() {
    let mut parser = EventSourceParser::new();
    parser.feed(&id_line(MAX_ID_BYTES));

    let events = parser.feed(&b"data\n\n".repeat(100));
    assert_eq!(parser.oversized_fields_ignored(), 0);
    let first = events.first().and_then(|event| event.id.clone());
    assert_eq!(first.as_deref().map(str::len), Some(MAX_ID_BYTES));
    for event in &events {
        assert!(
            matches!((&first, &event.id), (Some(first), Some(id)) if Arc::ptr_eq(first, id)),
            "every event shares one id allocation"
        );
    }
}

#[test]
fn a_huge_event_type_is_ignored_and_counted() {
    let mut parser = EventSourceParser::new();
    let mut stream = b"event: ".to_vec();
    stream.resize(stream.len() + MAX_FIELD_BYTES + 1, b'e');
    stream.extend_from_slice(b"\ndata: x\n\n");

    let events = parser.feed(&stream);
    assert_eq!(data_of(&events), vec!["x"]);
    assert_eq!(events.first().and_then(|event| event.event.clone()), None);
    assert_eq!(parser.oversized_fields_ignored(), 1);
}

#[test]
fn an_id_inside_an_unterminated_event_does_not_become_the_last_event_id() {
    let mut parser = EventSourceParser::new();
    parser.feed(b"id: 41\ndata: a\n\nid: 42\ndata: x\n");
    assert_eq!(
        parser.last_event_id(),
        "41",
        "not committed before the blank line"
    );

    assert_eq!(parser.finish(), vec![]);
    assert_eq!(
        parser.last_event_id(),
        "41",
        "a cut stream keeps the last dispatched id"
    );

    // After a reconnect the dropped block's id must not resurface either.
    let events = parser.feed(b"data: y\n\n");
    assert_eq!(
        events.first().and_then(|event| event.id.as_deref()),
        Some("41")
    );
}

#[test]
fn a_blank_line_commits_the_id_even_without_data() {
    let mut parser = EventSourceParser::new();
    assert_eq!(parser.feed(b"id: 5\n\n"), vec![]);
    assert_eq!(parser.last_event_id(), "5");
}

#[test]
fn finish_lets_the_parser_strip_a_bom_on_the_next_stream() {
    let mut parser = EventSourceParser::new();
    assert_eq!(data_of(&parser.feed(b"\xef\xbb\xbfdata: a\n\n")), vec!["a"]);
    parser.finish();
    assert_eq!(data_of(&parser.feed(b"\xef\xbb\xbfdata: b\n\n")), vec!["b"]);
}
