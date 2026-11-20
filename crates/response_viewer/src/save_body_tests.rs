//! `response::SaveBody` lands the body on disk (REQ-RESP-07). The save is driven the
//! way the app drives it — the action dispatched at the viewer, the platform save dialog
//! answered with the destination, the background write left to land — and nothing but
//! that write touches the file, so a no-op save fails the test.

use gpui::{Focusable as _, TestAppContext};
use http_engine::{Response, ResponseBody};
use pustman_actions::response::SaveBody;
use ui::test_support::{draw_window, init_test};

use crate::ResponseView;

#[gpui::test]
fn save_body_writes_file(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    let body: &[u8] = b"{\n  \"ok\": true\n}\n";
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(200, "OK", ResponseBody::Bytes(body.to_vec())),
            cx,
        )
    });
    let focus_handle = view.read_with(cx, |view, cx| view.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);

    // The dialog's destination is the only thing the test chooses, as the user would.
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let target = directory.path().join("saved-body.bin");

    cx.dispatch_action(SaveBody);
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();

    let saved = std::fs::read(&target).expect("the save must write the file");
    assert_eq!(
        saved, body,
        "the file holds the response body's exact bytes"
    );
}
