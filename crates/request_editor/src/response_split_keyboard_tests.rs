//! The split handle's keyboard resize (#187): Up and Down on the focused handle move
//! the boundary between the request and response panes — the keyboard's side of the
//! drag. Until now that path was verified by inspection only (S29e review 1).
//!
//! `Decision:` the test reads the handle's on-screen position rather than the split
//! fraction: where the handle is drawn is what the user sees move (S02/R02's
//! "resizable request/response split"), and it drives the same keystroke dispatch —
//! [`gpui::Interactivity::on_key_down`] on the focused handle — the app uses, so the
//! keyboard-first path (a tab stop with an accessible splitter role) is the one under
//! test.

use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px, size};
use ui::test_support::{draw_window, init_test, press_key};

use crate::RequestEditor;

use super::{KEYBOARD_STEP, SPLIT_HANDLE_SELECTOR};

/// A request tab on the app's real init order, tall enough that neither pane runs into
/// its clamp during the test — the clamps must not hide a wrong step size.
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(crate::new_request::fresh_request(), window, cx)
    });
    cx.update(|window, _cx| window.activate_window());
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();
    draw_window(cx);
    (editor, cx)
}

/// The handle's painted bounds: the request pane's bottom edge and the response pane's
/// top edge at once.
fn handle_bounds(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(SPLIT_HANDLE_SELECTOR)
        .expect("the split handle is painted")
}

#[gpui::test]
fn test_up_and_down_on_the_split_handle_resize_the_panes(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);

    // The handle is a tab stop with its own focus handle — this is where its key-down
    // listener sits (key handlers register when the focused frame paints).
    let focus = cx.read(|cx| editor.read_with(cx, |editor, _| editor.split_focus.clone()));
    cx.update(|window, cx| window.focus(&focus, cx));
    draw_window(cx);

    let split_height = cx.update(|window, _cx| window.viewport_size()).height;
    let step = split_height * KEYBOARD_STEP;
    let before = handle_bounds(cx);

    press_key(cx, "down");
    cx.run_until_parked();
    draw_window(cx);

    let after_down = handle_bounds(cx);
    let grown = after_down.origin.y - before.origin.y;
    assert!(
        grown > px(0.),
        "Down moves the handle down: the request pane grows and the response pane shrinks"
    );
    assert!(
        (grown - step).abs() <= px(1.),
        "Down moves the handle exactly one step of the split's height (step {step:?}, \
         moved {grown:?})"
    );

    press_key(cx, "up");
    cx.run_until_parked();
    draw_window(cx);

    let after_up = handle_bounds(cx);
    assert!(
        (after_up.origin.y - before.origin.y).abs() <= px(1.),
        "Up moves the handle back one step: the panes return to where they were \
         (before {before:?}, after {after_up:?})"
    );
}
