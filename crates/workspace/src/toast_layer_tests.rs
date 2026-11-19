//! `workspace.show_toast` puts a toast on screen and takes it off again: a report of
//! good news dismisses itself when its time is up, an error stays until the user sends
//! it away, and the action button runs the handler it was given. A stub that showed
//! toasts and never removed them fails here.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, Window};
use ui::components::toast::{toast_action_selector, toast_message_selector};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::toast_layer::{TOAST_DURATION, ToastAction};
use crate::{Workspace, init};

/// A workspace in its own window, on the app's real init order and the real default
/// keymap (a local copy of `workspace_tests.rs`'s helper — existing tests stay read-only).
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    cx.add_window_view(Workspace::new)
}

/// Show one toast through the workspace and return its handle.
fn show_toast(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    message: &'static str,
    level: Severity,
    action: Option<ToastAction>,
) -> usize {
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.show_toast(message, level, action, cx)
        })
    })
}

/// How many toasts are on screen.
fn toasts_len(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    cx.read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
}

/// Whether `outer` fully contains `inner` (the `workspace_tests.rs` shape).
fn contains_bounds(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height
}

#[gpui::test]
fn toast_shows_and_dismisses(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    show_toast(&workspace, cx, "Collection saved", Severity::Info, None);
    draw_window(cx);

    // The message key is in the frame only while the message renders, so this asserts
    // the message itself — not just that some toast exists.
    let message_selector: &'static str = toast_message_selector("Collection saved").leak();
    let message_bounds = cx
        .debug_bounds(message_selector)
        .expect("the toast's message renders");
    let window_bounds = cx.update(|window, _| window.bounds());
    assert!(
        contains_bounds(window_bounds, message_bounds),
        "the toast renders inside the window"
    );
    let status_bar = cx
        .debug_bounds("status-bar")
        .expect("the status bar renders");
    assert!(
        message_bounds.origin.y + message_bounds.size.height <= status_bar.origin.y,
        "the toast sits above the status bar"
    );
    assert_eq!(
        toasts_len(&workspace, cx),
        1,
        "the stack holds the one toast"
    );

    cx.executor()
        .advance_clock(TOAST_DURATION + Duration::from_millis(100));
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(message_selector).is_none(),
        "the toast dismissed itself when its time was up"
    );
    assert_eq!(toasts_len(&workspace, cx), 0, "the stack is empty again");
}

#[gpui::test]
fn error_toast_stays_until_dismissed(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);

    let handle = show_toast(&workspace, cx, "Request failed", Severity::Error, None);
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector("Request failed").leak();

    cx.executor().advance_clock(TOAST_DURATION * 10);
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(message_selector).is_some(),
        "an error is still on screen long past the auto-dismiss time"
    );
    assert_eq!(toasts_len(&workspace, cx), 1, "the stack still holds it");

    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| workspace.dismiss_toast(handle, cx));
    });
    draw_window(cx);

    assert!(
        cx.debug_bounds(message_selector).is_none(),
        "the toast goes when the user dismisses it"
    );
    assert_eq!(toasts_len(&workspace, cx), 0, "the stack is empty again");
}

#[gpui::test]
fn toast_action_runs_its_handler(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let handled = Rc::new(Cell::new(false));
    let action = ToastAction {
        label: SharedString::new_static("Undo"),
        handler: {
            let handled = handled.clone();
            Rc::new(move |_window: &mut Window, _cx: &mut App| handled.set(true))
        },
    };

    show_toast(&workspace, cx, "Deleted", Severity::Info, Some(action));
    draw_window(cx);

    let action_selector: &'static str = toast_action_selector("Deleted").leak();
    let action_bounds = cx
        .debug_bounds(action_selector)
        .expect("the toast's action button renders");
    cx.simulate_click(action_bounds.center(), Modifiers::none());

    assert!(
        handled.get(),
        "clicking the toast's action button runs its handler"
    );
}
