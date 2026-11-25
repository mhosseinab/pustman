//! The Message log's review fixes (S51c review): keys meant for a control inside the log
//! stay with that control, a spilled payload too large to open says so instead of
//! offering itself, and a very long line is cut where its detail is built.

use api_model::{Message, Payload, ProtocolFields, SecretValues, SessionEvent, SessionMessageKind};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use crate::session_log::{SessionLog, TimeMode};
use crate::session_store::SessionStore;

fn message(kind: SessionMessageKind, payload: Payload, size: u64) -> SessionEvent {
    SessionEvent::Received(Message {
        at: 1_700_000_000_000,
        kind,
        size,
        payload,
        fields: ProtocolFields::None,
    })
}

/// A store holding `events`; none of them spills, so no tokio runtime is needed beyond
/// a current-thread one to drive the async push.
fn open_log(
    events: Vec<SessionEvent>,
    cx: &mut TestAppContext,
) -> (Entity<SessionLog>, &mut VisualTestContext) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test's tokio runtime must build");
    let mut store = SessionStore::new(SecretValues::new());
    runtime.block_on(async {
        for event in events {
            store.push(event).await.expect("the push must succeed");
        }
    });
    cx.update(init_test);
    let (log, cx) = cx.add_window_view(|_, cx| {
        let store = cx.new(|_| store);
        SessionLog::new(store, cx)
    });
    draw_window(cx);
    (log, cx)
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

fn detail_text(log: &Entity<SessionLog>, seq: usize, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| log.read(cx).detail_text(seq))
        .unwrap_or_else(|| panic!("event {seq} must be expanded"))
}

#[gpui::test]
fn enter_on_a_button_inside_the_log_does_not_collapse_the_selected_row(cx: &mut TestAppContext) {
    let payload = b"hello".to_vec();
    let (log, cx) = open_log(
        vec![message(
            SessionMessageKind::Text,
            Payload::Inline(payload),
            5,
        )],
        cx,
    );
    click_row(&log, 0, cx);
    assert!(cx.read(|cx| log.read(cx).is_expanded(0)));

    // Tab from the list to the header's time toggle, then press Enter on it.
    let handle = cx.read(|cx| log.read(cx).focus_handle(cx));
    cx.update(|window, cx| {
        window.focus(&handle, cx);
        window.focus_next(cx);
    });
    assert!(
        !cx.update(|window, _| handle.is_focused(window)),
        "focus moved off the list onto a control inside it"
    );
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| log.read(cx).time_mode()),
        TimeMode::Relative,
        "Enter activates the focused button"
    );
    assert!(
        cx.read(|cx| log.read(cx).is_expanded(0)),
        "Enter meant for the button never reaches the list's row toggle"
    );
}

#[gpui::test]
fn a_spilled_payload_over_the_cap_says_it_is_too_large_to_open(cx: &mut TestAppContext) {
    let len = 512 * 1024 * 1024;
    let spilled = Payload::Spilled {
        path: std::env::temp_dir().join("pustman-session-log-never-read"),
        len,
        prefix: vec![0xab; 64],
    };
    let (log, cx) = open_log(vec![message(SessionMessageKind::Binary, spilled, len)], cx);
    click_row(&log, 0, cx);

    let lines = detail_text(&log, 0, cx);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Too large to open in the log")),
        "an over-cap payload says why it cannot be opened; got {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "Open full payload"),
        "an over-cap payload offers no button that would load it; got {lines:?}"
    );
}

#[gpui::test]
fn a_very_long_line_is_cut_with_a_note_of_what_was_left_out(cx: &mut TestAppContext) {
    let text = "x".repeat(10_000);
    let (log, cx) = open_log(
        vec![message(
            SessionMessageKind::Text,
            Payload::Inline(text.into_bytes()),
            10_000,
        )],
        cx,
    );
    click_row(&log, 0, cx);

    let lines = detail_text(&log, 0, cx);
    let line = lines
        .iter()
        .find(|line| line.starts_with("xxxx"))
        .unwrap_or_else(|| panic!("the payload line is shown; got {lines:?}"));
    assert!(
        line.chars().count() < 4_100,
        "the line is cut to a bounded length; it is {} characters",
        line.chars().count()
    );
    assert!(
        line.ends_with("6000 more characters)"),
        "the cut line says how much was left out; it ends {:?}",
        line.chars().rev().take(40).collect::<String>()
    );
}
