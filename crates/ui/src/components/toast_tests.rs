//! A toast with a timeout leaves the stack when its time is up — on the GPUI clock, never
//! wall time — while one without a timeout stays until the user dismisses it. A toast
//! dismissed by hand goes at once and its timer never speaks again.

use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::components::toast::{Toast, ToastEvent, Toasts};
use crate::test_support::init_test;

/// An empty stack of toasts in its own window.
fn stack_with(cx: &mut TestAppContext) -> (Entity<Toasts>, &mut VisualTestContext) {
    cx.add_window_view(|_, cx| Toasts::new(cx))
}

/// Every [`ToastEvent`] the stack has emitted since the last call.
fn drain(events: &mut UnboundedReceiver<ToastEvent>) -> Vec<ToastEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

/// How many toasts are on screen.
fn len(cx: &mut VisualTestContext, toasts: &Entity<Toasts>) -> usize {
    cx.read(|cx| toasts.read_with(cx, |toasts, _| toasts.len()))
}

#[gpui::test]
fn toast_auto_dismisses_after_timeout(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (toasts, cx) = stack_with(cx);
    let mut events = cx.events(&toasts);

    let (timed, untimed) = toasts.update(cx, |toasts, cx| {
        let timed = toasts.push(
            Toast::new("saved", "Saved"),
            Some(Duration::from_secs(5)),
            cx,
        );
        let untimed = toasts.push(Toast::new("failed", "Request failed"), None, cx);
        (timed, untimed)
    });
    assert_eq!(len(cx, &toasts), 2, "both toasts are on screen");
    assert_eq!(drain(&mut events), Vec::new(), "pushing dismisses nothing");

    // Before its timeout the timed toast is still on screen.
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(
        len(cx, &toasts),
        2,
        "four seconds in, the five-second toast is still on screen"
    );
    assert_eq!(drain(&mut events), Vec::new(), "nothing has been dismissed");

    // Past its timeout the timed toast dismissed itself.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(len(cx, &toasts), 1, "the timed toast left the stack");
    assert_eq!(
        drain(&mut events),
        vec![ToastEvent::Dismissed { handle: timed }],
        "the timer announced the toast it dismissed"
    );

    // The toast left behind is the untimed one: taking it off the stack empties it.
    toasts.update(cx, |toasts, cx| toasts.dismiss(untimed, cx));
    assert_eq!(
        len(cx, &toasts),
        0,
        "the untimed toast stayed until dismissed"
    );
    assert_eq!(
        drain(&mut events),
        vec![ToastEvent::Dismissed { handle: untimed }]
    );

    // Dismissing a timed toast by hand cancels its timer: it never fires a second time.
    let early = toasts.update(cx, |toasts, cx| {
        toasts.push(
            Toast::new("copied", "Copied"),
            Some(Duration::from_secs(5)),
            cx,
        )
    });
    toasts.update(cx, |toasts, cx| toasts.dismiss(early, cx));
    assert_eq!(
        drain(&mut events),
        vec![ToastEvent::Dismissed { handle: early }],
        "the manual dismissal is the one event"
    );
    cx.executor().advance_clock(Duration::from_secs(10));
    cx.run_until_parked();
    assert_eq!(
        drain(&mut events),
        Vec::new(),
        "the cancelled timer never dismisses a second time"
    );
}
