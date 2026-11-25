//! The Message log (REQ-LOG-01/02, REQ-WS-04): a session's events as a virtualized list
//! of one-line rows, any of which expands into its full payload — JSON pretty-printed,
//! binary as hexdump rows — and its protocol fields.

use std::sync::Arc;

use api_model::{
    ConnectionDetails, Message, Payload, ProtocolFields, SecretValues, SessionEvent,
    SessionMessageKind,
};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::SessionLog;
use crate::hex_view::{BYTES_PER_ROW, HexRows};
use crate::pretty::Formatter;
use crate::session_store::{PREVIEW_LEN, SPILL_THRESHOLD, SessionStore};

/// The pushes may spill through `http_engine::SpillWriter`, whose writes are tokio
/// futures, so the store is filled on a tokio runtime of its own (the
/// `session_store_tests` shape).
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test's tokio runtime must build")
}

fn message(kind: SessionMessageKind, payload: Vec<u8>, at: u64) -> Message {
    Message {
        at,
        kind,
        size: payload.len() as u64,
        payload: Payload::Inline(payload),
        fields: ProtocolFields::None,
    }
}

fn connected() -> SessionEvent {
    SessionEvent::Connected {
        details: ConnectionDetails {
            request_headers: Vec::new(),
            response_status: Some(101),
            response_headers: Vec::new(),
            protocol: None,
        },
    }
}

fn store_with(runtime: &tokio::runtime::Runtime, events: Vec<SessionEvent>) -> SessionStore {
    let mut store = SessionStore::new(SecretValues::new());
    runtime.block_on(async {
        for event in events {
            store.push(event).await.expect("the push must succeed");
        }
    });
    store
}

fn open_log(
    store: SessionStore,
    cx: &mut TestAppContext,
) -> (
    Entity<SessionLog>,
    Entity<SessionStore>,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    let (log, cx) = cx.add_window_view(|_, cx| {
        let store = cx.new(|_| store);
        SessionLog::new(store, cx)
    });
    let store = cx.read(|cx| log.read(cx).store().clone());
    draw_window(cx);
    (log, store, cx)
}

/// Whether the summary row of the event with sequence number `seq` is on screen.
fn row_is_drawn(log: &Entity<SessionLog>, seq: usize, cx: &mut VisualTestContext) -> bool {
    let id = cx.read(|cx| log.read(cx).row_element_id(seq));
    let selector: &'static str = Box::leak(id.to_string().into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

/// Whether line `line` of the event's expanded detail is on screen.
fn detail_line_is_drawn(
    log: &Entity<SessionLog>,
    seq: usize,
    line: usize,
    cx: &mut VisualTestContext,
) -> bool {
    let id = cx.read(|cx| log.read(cx).detail_line_element_id(seq, line));
    let selector: &'static str = Box::leak(id.to_string().into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

fn detail_text(log: &Entity<SessionLog>, seq: usize, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| log.read(cx).detail_text(seq))
        .unwrap_or_else(|| panic!("event {seq} must be expanded"))
}

fn click_row(log: &Entity<SessionLog>, seq: usize, cx: &mut VisualTestContext) {
    let id = cx.read(|cx| log.read(cx).row_element_id(seq));
    let selector: &'static str = Box::leak(id.to_string().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("row {seq} must be on screen to be clicked"));
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// Where the needle lines sit, contiguous, inside the haystack.
fn position_of(haystack: &[String], needle: &[String]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[gpui::test]
fn hundred_thousand_events_render_only_visible_rows(cx: &mut TestAppContext) {
    const EVENTS: usize = 100_000;
    let runtime = runtime();
    let events = (0..EVENTS)
        .map(|index| {
            let payload = format!("message {index}").into_bytes();
            let message = message(
                SessionMessageKind::Text,
                payload,
                1_700_000_000_000 + index as u64,
            );
            if index % 2 == 0 {
                SessionEvent::Sent(message)
            } else {
                SessionEvent::Received(message)
            }
        })
        .collect();
    let (log, store, cx) = open_log(store_with(&runtime, events), cx);

    let drawn: Vec<usize> = (0..300)
        .filter(|&seq| row_is_drawn(&log, seq, cx))
        .collect();
    assert!(
        (2..=128).contains(&drawn.len()),
        "only the rows the window can show may render; {} did",
        drawn.len()
    );
    assert_eq!(
        drawn.first(),
        Some(&0),
        "the top row is on screen and rendered"
    );
    assert_eq!(
        drawn.last().map(|last| last + 1),
        Some(drawn.len()),
        "the rendered rows are one contiguous window from the top"
    );
    assert!(
        !row_is_drawn(&log, 50_000, cx),
        "a row 50,000 down is far outside the viewport and must not render at all"
    );
    assert!(
        !row_is_drawn(&log, EVENTS - 1, cx),
        "the last row is far outside the viewport and must not render at all"
    );

    // A new row arrives: the log follows it to the bottom.
    store.update(cx, |store, cx| {
        runtime
            .block_on(store.push(SessionEvent::Received(message(
                SessionMessageKind::Text,
                b"the newest".to_vec(),
                1_700_000_100_000,
            ))))
            .expect("the push must succeed");
        cx.notify();
    });
    cx.run_until_parked();
    draw_window(cx);
    draw_window(cx);
    assert!(
        row_is_drawn(&log, EVENTS, cx),
        "auto-scroll brings the new row on screen"
    );
    assert!(
        !row_is_drawn(&log, 0, cx),
        "following the new row scrolled the top of the log away"
    );
}

#[gpui::test]
fn expanding_json_row_pretty_prints(cx: &mut TestAppContext) {
    let runtime = runtime();
    let json = br#"{"user":{"id":7,"tags":["a","b"]},"ok":true}"#.to_vec();
    let events = vec![
        connected(),
        SessionEvent::Received(message(
            SessionMessageKind::Json,
            json.clone(),
            1_700_000_000_000,
        )),
    ];
    let (log, _store, cx) = open_log(store_with(&runtime, events), cx);
    assert!(
        cx.read(|cx| log.read(cx).detail_text(1)).is_none(),
        "nothing is formatted before the row is expanded"
    );

    // From the keyboard: focus the log, walk down to the JSON row and press Enter.
    let handle = cx.read(|cx| log.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    press_key(cx, "down");
    press_key(cx, "down");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    let expected: Vec<String> = Formatter::Json
        .format(&json)
        .expect("the fixture is valid JSON")
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(expected.len() > 5, "the pretty form spans many lines");
    let lines = detail_text(&log, 1, cx);
    let start = position_of(&lines, &expected).unwrap_or_else(|| {
        panic!("the expanded row shows the payload pretty-printed; it shows {lines:?}")
    });
    assert!(
        detail_line_is_drawn(&log, 1, start + expected.len() - 1, cx),
        "the pretty-printed lines are drawn under the row"
    );
    assert!(
        !cx.read(|cx| log.read(cx).is_expanded(0)),
        "only the row Enter was pressed on expands"
    );

    // Enter again collapses it.
    press_key(cx, "enter");
    draw_window(cx);
    assert!(
        cx.read(|cx| log.read(cx).detail_text(1)).is_none(),
        "Enter on an expanded row collapses it"
    );
}

#[gpui::test]
fn binary_row_expands_to_hex(cx: &mut TestAppContext) {
    let runtime = runtime();
    let bytes: Vec<u8> = (0u8..40).collect();
    let big: Vec<u8> = (0..SPILL_THRESHOLD + 3 * BYTES_PER_ROW)
        .map(|index| (index % 251) as u8)
        .collect();
    let events = vec![
        SessionEvent::Received(message(SessionMessageKind::Binary, bytes.clone(), 1)),
        SessionEvent::Received(message(SessionMessageKind::Binary, big.clone(), 2)),
    ];
    let (log, _store, cx) = open_log(store_with(&runtime, events), cx);

    click_row(&log, 0, cx);
    let rows = HexRows::new(Arc::new(bytes));
    let expected: Vec<String> = (0..rows.row_count())
        .map(|index| {
            let row = rows.row(index).expect("the row exists");
            format!("{}  {}  {}", row.offset, row.hex, row.ascii)
        })
        .collect();
    let lines = detail_text(&log, 0, cx);
    let start = position_of(&lines, &expected)
        .unwrap_or_else(|| panic!("a binary payload expands to hexdump rows; got {lines:?}"));
    assert!(
        detail_line_is_drawn(&log, 0, start, cx),
        "the hexdump is drawn under the row"
    );

    // A spilled binary shows its in-memory prefix as hex and offers the whole payload,
    // read off the foreground when asked for.
    click_row(&log, 1, cx);
    let lines = detail_text(&log, 1, cx);
    assert!(
        lines.iter().any(|line| line == "Open full payload"),
        "a spilled payload offers the full payload; got {lines:?}"
    );
    let prefix_rows = PREVIEW_LEN.div_ceil(BYTES_PER_ROW);
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("0000")).count(),
        prefix_rows,
        "until it is opened, only the prefix's rows are shown"
    );
    log.update(cx, |log, cx| log.open_full_payload(1, cx));
    cx.run_until_parked();
    draw_window(cx);
    let lines = detail_text(&log, 1, cx);
    let full_rows = HexRows::new(Arc::new(big));
    let last = full_rows
        .row(full_rows.row_count() - 1)
        .expect("the last row exists");
    assert!(
        lines
            .iter()
            .any(|line| *line == format!("{}  {}  {}", last.offset, last.hex, last.ascii)),
        "the opened payload shows every byte, through the last row"
    );
    assert!(
        !lines.iter().any(|line| line == "Open full payload"),
        "the opened payload no longer offers itself"
    );
}

#[gpui::test]
fn socketio_row_shows_event_name_and_ack_id(cx: &mut TestAppContext) {
    let runtime = runtime();
    let payload = br#"["hello",{"room":"lobby"}]"#.to_vec();
    let mut event = message(SessionMessageKind::Event, payload, 1_700_000_000_000);
    event.fields = ProtocolFields::SocketIo {
        event: "chat:message".to_owned(),
        ack_id: Some(42),
    };
    let (log, _store, cx) = open_log(
        store_with(&runtime, vec![SessionEvent::Received(event)]),
        cx,
    );

    let summary = cx
        .read(|cx| log.read(cx).summary(0, cx))
        .expect("the row has a summary");
    assert_eq!(summary.direction.as_ref(), "Received");
    assert!(
        summary.preview.contains("chat:message"),
        "the one-line preview names the Socket.IO event; got {:?}",
        summary.preview
    );

    click_row(&log, 0, cx);
    let lines = detail_text(&log, 0, cx);
    assert!(
        lines.iter().any(|line| line == "Event: chat:message"),
        "the expanded row shows the event name; got {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "Ack ID: 42"),
        "the expanded row shows the ack id; got {lines:?}"
    );
}
