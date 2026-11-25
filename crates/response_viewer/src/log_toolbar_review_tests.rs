//! The Message log toolbar's review fixes (S51d review): a snapshot keeps the spill files
//! a Clear would delete until the export is done with them, focus stays on the Pause /
//! Resume button when it flips, and Escape in the clear confirmation keeps the messages.

use api_model::{Message, Payload, ProtocolFields, SecretValues, SessionEvent, SessionMessageKind};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};
use pustman_actions::message_log::{Clear, ConfirmClear};
use ui::test_support::{draw_window, init_test, press_key};

use crate::session_log::SessionLog;
use crate::session_store::{SPILL_THRESHOLD, SessionStore};

fn received(payload: Vec<u8>) -> SessionEvent {
    SessionEvent::Received(Message {
        at: 1_700_000_000_000,
        kind: SessionMessageKind::Text,
        size: payload.len() as u64,
        payload: Payload::Inline(payload),
        fields: ProtocolFields::None,
    })
}

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
    let (log, cx) = cx.add_window_view(|window, cx| {
        let store = cx.new(|_| store);
        SessionLog::new_in(store, window, cx)
    });
    let handle = cx.read(|cx| log.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    draw_window(cx);
    (log, cx)
}

#[gpui::test]
fn a_snapshot_keeps_spill_files_alive_across_clear_until_it_drops(cx: &mut TestAppContext) {
    let big = vec![b'a'; SPILL_THRESHOLD + 1];
    let (log, cx) = open_log(vec![received(big)], cx);
    let store = cx.read(|cx| log.read(cx).store().clone());
    let path = cx
        .read(|cx| match store.read(cx).events().first() {
            Some(SessionEvent::Received(Message {
                payload: Payload::Spilled { path, .. },
                ..
            })) => Some(path.clone()),
            _ => None,
        })
        .expect("the big payload is spilled");

    let snapshot = cx.read(|cx| store.read(cx).snapshot());
    cx.dispatch_action(Clear);
    cx.dispatch_action(ConfirmClear);
    cx.run_until_parked();
    assert!(cx.read(|cx| store.read(cx).is_empty()));
    assert!(path.exists(), "the snapshot still holds the spill file");

    let mut exported = Vec::new();
    snapshot
        .export_jsonl(&mut exported)
        .expect("the snapshot exports after the clear");
    let text = String::from_utf8(exported).expect("the export is UTF-8");
    assert_eq!(text.lines().count(), 1, "one line for the one event");

    drop(snapshot);
    assert!(
        !path.exists(),
        "the last holder dropping deletes the spill file"
    );
}

#[gpui::test]
fn pause_keeps_focus_on_the_pause_button(cx: &mut TestAppContext) {
    let (log, cx) = open_log(vec![received(b"hello".to_vec())], cx);
    let handle = cx.read(|cx| log.read(cx).pause_button_focus_handle().clone());
    cx.update(|window, cx| window.focus(&handle, cx));
    draw_window(cx);

    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    assert!(cx.read(|cx| log.read(cx).is_paused()));
    assert!(
        cx.update(|window, _| handle.is_focused(window)),
        "focus stays on the button after it turns into Resume"
    );

    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    assert!(!cx.read(|cx| log.read(cx).is_paused()));
    assert!(cx.update(|window, _| handle.is_focused(window)));
}

#[gpui::test]
fn escape_in_the_clear_confirmation_keeps_the_messages(cx: &mut TestAppContext) {
    let (log, cx) = open_log(vec![received(b"hello".to_vec())], cx);
    cx.dispatch_action(Clear);
    cx.run_until_parked();
    draw_window(cx);
    assert!(cx.read(|cx| log.read(cx).is_confirming_clear()));

    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(!cx.read(|cx| log.read(cx).is_confirming_clear()));
    assert_eq!(cx.read(|cx| log.read(cx).store().read(cx).len()), 1);
}
