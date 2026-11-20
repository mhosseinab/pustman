//! The response-viewer wire (S29w): the app's init order must plug `response_viewer` in
//! and the default keymap must reach it, so `response::Find` on the real `ctrl-f` binding
//! opens the find bar over a response body — the whole app-level chain, not a direct
//! dispatch into the crate. `Decision:` `response_viewer` exposes no find-state accessor
//! (`FindBar` is crate-private and `ResponseView` has no body getter), and this step may
//! not edit that crate, so the test asserts the nearest public observable: the keystroke
//! moves focus off the viewer's own element onto a focusable child of it — the find bar's
//! query field, which `BodyView::open_find` focuses (REQ-RESP-03).

use gpui::{Focusable, TestAppContext};
use http_engine::{Response, ResponseBody};
use pustman::StoreLocation;
use response_viewer::ResponseView;
use ui::test_support::draw_window;

#[gpui::test]
fn wire_response_viewer_init_registers_actions(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (view, cx) = cx.add_window_view(|_window, cx| ResponseView::new(cx));
    // The local store opens on a background task; let it land before the window draws.
    cx.run_until_parked();

    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(
                200,
                "OK",
                ResponseBody::Bytes(b"hello brave world".to_vec()),
            ),
            cx,
        );
    });
    let view_focus = cx.read(|cx| view.read_with(cx, |view, cx| view.focus_handle(cx)));
    cx.update(|window, cx| view_focus.focus(window, cx));
    draw_window(cx);
    cx.run_until_parked();

    assert!(
        cx.update(|window, _| view_focus.is_focused(window)),
        "the response view has focus before the keystroke, so the keystroke lands in its key \
         context"
    );

    // The real binding from the bundled default keymap (`assets/keymaps/`): a direct
    // dispatch would not prove the app's keymap load reaches the viewer.
    let find_keystroke = if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    };
    cx.simulate_keystrokes(find_keystroke);
    draw_window(cx);
    cx.run_until_parked();

    let (own_focused, child_focused) = cx.update(|window, cx| {
        (
            view_focus.is_focused(window),
            view_focus.contains_focused(window, cx),
        )
    });
    assert!(
        !own_focused && child_focused,
        "{find_keystroke} moves focus off the viewer's own element onto a focusable child of it \
         — the find bar's query field, the one thing response::Find focuses (REQ-RESP-03); \
         own_focused: {own_focused}, child_focused: {child_focused}"
    );
}
