//! The Message log toolbar (REQ-LOG-03): search narrows the listed rows, Pause holds the
//! scroll position while events keep arriving, Clear empties the log after its inline
//! confirmation, and Export writes one JSON line per event through the save dialog.

use api_model::{Message, Payload, ProtocolFields, SecretValues, SessionEvent, SessionMessageKind};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use pustman_actions::message_log::{Clear, ConfirmClear, Export, Pause, Resume};
use ui::test_support::{draw_window, init_test};

use super::SEARCH_DEBOUNCE;
use crate::session_log::SessionLog;
use crate::session_store::SessionStore;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test's tokio runtime must build")
}

fn text_event(index: usize) -> SessionEvent {
    let payload = format!("message {index}").into_bytes();
    let message = Message {
        at: 1_700_000_000_000 + index as u64,
        kind: SessionMessageKind::Text,
        size: payload.len() as u64,
        payload: Payload::Inline(payload),
        fields: ProtocolFields::None,
    };
    if index.is_multiple_of(2) {
        SessionEvent::Sent(message)
    } else {
        SessionEvent::Received(message)
    }
}

fn open_log<'a>(
    events: usize,
    runtime: &tokio::runtime::Runtime,
    cx: &'a mut TestAppContext,
) -> (
    Entity<SessionLog>,
    Entity<SessionStore>,
    &'a mut VisualTestContext,
) {
    let mut store = SessionStore::new(SecretValues::new());
    runtime.block_on(async {
        for index in 0..events {
            store
                .push(text_event(index))
                .await
                .expect("the push must succeed");
        }
    });
    cx.update(init_test);
    let (log, cx) = cx.add_window_view(|window, cx| {
        let store = cx.new(|_| store);
        SessionLog::new_in(store, window, cx)
    });
    let store = cx.read(|cx| log.read(cx).store().clone());
    let handle = cx.read(|cx| log.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    draw_window(cx);
    (log, store, cx)
}

/// Push events `range` the way a live session does: into the store, which notifies.
fn push_events(
    store: &Entity<SessionStore>,
    range: std::ops::Range<usize>,
    runtime: &tokio::runtime::Runtime,
    cx: &mut VisualTestContext,
) {
    store.update(cx, |store, cx| {
        for index in range {
            runtime
                .block_on(store.push(text_event(index)))
                .expect("the push must succeed");
        }
        cx.notify();
    });
    cx.run_until_parked();
    draw_window(cx);
}

fn row_is_drawn(log: &Entity<SessionLog>, seq: usize, cx: &mut VisualTestContext) -> bool {
    let id = cx.read(|cx| log.read(cx).row_element_id(seq));
    let selector: &'static str = Box::leak(id.to_string().into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn search_narrows_rows(cx: &mut TestAppContext) {
    let runtime = runtime();
    let (log, store, cx) = open_log(20, &runtime, cx);
    assert!(row_is_drawn(&log, 0, cx));

    let search = cx
        .read(|cx| log.read(cx).search_input().cloned())
        .expect("a log built in a window has its search field");
    search.update(cx, |search, cx| search.set_text("message 13", cx));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| store.read(cx).filtered().len()),
        20,
        "the search waits for typing to pause"
    );
    cx.executor().advance_clock(SEARCH_DEBOUNCE);
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| store.read(cx).filtered().to_vec()),
        vec![13],
        "only the matching event is listed"
    );
    assert!(row_is_drawn(&log, 13, cx), "the match is on screen");
    assert!(!row_is_drawn(&log, 0, cx), "a non-matching row is gone");

    search.update(cx, |search, cx| search.set_text("", cx));
    cx.executor().advance_clock(SEARCH_DEBOUNCE);
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(cx.read(|cx| store.read(cx).filtered().len()), 20);
}

#[gpui::test]
fn pause_keeps_scroll_position_while_events_arrive(cx: &mut TestAppContext) {
    let runtime = runtime();
    let (log, store, cx) = open_log(10, &runtime, cx);

    // Following: new rows pull the list to the bottom.
    push_events(&store, 10..200, &runtime, cx);
    assert!(row_is_drawn(&log, 199, cx), "the log follows new rows");
    assert!(!row_is_drawn(&log, 0, cx));

    cx.dispatch_action(Pause);
    cx.run_until_parked();
    assert!(cx.read(|cx| log.read(cx).is_paused()));
    push_events(&store, 200..400, &runtime, cx);

    assert_eq!(
        cx.read(|cx| store.read(cx).len()),
        400,
        "events keep arriving while paused"
    );
    assert!(
        row_is_drawn(&log, 199, cx),
        "the rows on screen stay where they were"
    );
    assert!(
        !row_is_drawn(&log, 399, cx),
        "the list did not jump to the newest row"
    );

    cx.dispatch_action(Resume);
    cx.run_until_parked();
    draw_window(cx);
    assert!(!cx.read(|cx| log.read(cx).is_paused()));
    assert!(
        row_is_drawn(&log, 399, cx),
        "resuming jumps to the newest row"
    );
}

#[gpui::test]
fn clear_empties_log(cx: &mut TestAppContext) {
    let runtime = runtime();
    let (log, store, cx) = open_log(5, &runtime, cx);

    cx.dispatch_action(Clear);
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        cx.read(|cx| store.read(cx).len()),
        5,
        "Clear asks first: nothing is gone until it is confirmed"
    );
    assert!(cx.read(|cx| log.read(cx).is_confirming_clear()));

    cx.dispatch_action(ConfirmClear);
    cx.run_until_parked();
    draw_window(cx);
    assert!(cx.read(|cx| store.read(cx).is_empty()));
    assert!(!row_is_drawn(&log, 0, cx));
    assert!(!cx.read(|cx| log.read(cx).is_confirming_clear()));
}

#[gpui::test]
fn export_writes_one_line_per_event(cx: &mut TestAppContext) {
    let runtime = runtime();
    let (_log, _store, cx) = open_log(7, &runtime, cx);
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let target = directory.path().join("session.jsonl");

    cx.dispatch_action(Export);
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();

    let written = std::fs::read_to_string(&target).expect("the export must write the file");
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), 7, "one line per event");
    for (index, line) in lines.iter().enumerate() {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("every line is one JSON document");
        assert!(
            line.contains(&format!("message {index}")),
            "line {index} carries its event's payload: {value}"
        );
    }
}
