//! Clearing the response region without dropping the view behind it (#264).
//!
//! A new send used to swap in a fresh [`ResponseView`], which dropped the old entity
//! together with its in-flight save and copy tasks (a dropped `Task` is cancelled) — a
//! slow "save body" was lost the moment the user sent again. The clear keeps the view
//! alive: what it *shows* is reset, what it has *in flight* is not.

use gpui::{Focusable as _, TestAppContext};
use http_engine::{Response, ResponseBody};
use pustman_actions::response::SaveBody;
use ui::test_support::{draw_window, init_test};

use crate::{ResponseView, STATUS_LINE_SELECTOR};

/// The body the tests show and save: small enough to read in an assertion.
const BODY: &[u8] = b"{\n  \"ok\": true\n}\n";

#[gpui::test]
fn test_clear_keeps_an_in_flight_save_alive(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(200, "OK", ResponseBody::Bytes(BODY.to_vec())),
            cx,
        )
    });
    let focus_handle = view.read_with(cx, |view, cx| view.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);

    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let target = directory.path().join("saved-body.bin");

    // The save is in flight, waiting at the platform dialog, when the clear lands.
    cx.dispatch_action(SaveBody);
    view.update(cx, |view, cx| view.clear(cx));
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();

    let saved = std::fs::read(&target).expect("the save must survive the clear and write the file");
    assert_eq!(
        saved, BODY,
        "the file holds the response body's exact bytes"
    );
}

#[gpui::test]
fn test_clear_resets_the_shown_response(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(200, "OK", ResponseBody::Bytes(BODY.to_vec())),
            cx,
        )
    });
    draw_window(cx);
    assert!(
        cx.debug_bounds(STATUS_LINE_SELECTOR).is_some(),
        "the response's status line is on screen before the clear"
    );

    view.update(cx, |view, cx| view.clear(cx));
    draw_window(cx);

    let (holds_response, holds_error, body_left) = view.read_with(cx, |view, cx| {
        (
            view.response.is_some(),
            view.error.is_some(),
            !view.body.read(cx).lines().is_empty(),
        )
    });
    assert!(
        cx.debug_bounds(STATUS_LINE_SELECTOR).is_none(),
        "the previous response's status line is gone"
    );
    assert!(!holds_response, "the viewer keeps no response of its own");
    assert!(!holds_error, "a clear is not a failure: no error is shown");
    assert!(!body_left, "the body pane shows none of the previous body");
}
