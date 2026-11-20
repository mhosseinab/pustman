//! Raw mode shows the bytes as they arrived (REQ-RESP-02): the exact decoded text, no
//! formatting and no lossy conversion of valid UTF-8 — the same whether the body is in
//! memory or was streamed to disk and read back.

use std::io::Write as _;
use std::sync::Arc;

use gpui::TestAppContext;
use http_engine::{Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::BodyView;

#[gpui::test]
fn raw_mode_shows_exact_bytes(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));

    let body = b"{\n  \"ok\": true\n}\n";
    view.update(cx, |view, cx| {
        view.set_body(
            Arc::new(Response::new(200, "OK", ResponseBody::Bytes(body.to_vec()))),
            cx,
        )
    });
    draw_window(cx);

    let (lines, lossy) = shown(&view, cx);
    assert_eq!(
        lines,
        ["{", "  \"ok\": true", "}"],
        "raw mode keeps every byte of the text, including indentation"
    );
    assert!(!lossy, "valid UTF-8 is shown exactly, never lossily");

    // A body streamed to disk is read off the foreground and shown the same way.
    let mut file = tempfile::NamedTempFile::new().expect("the temp file must be created");
    file.write_all(b"from disk\nsecond line")
        .expect("the temp file must write");
    view.update(cx, |view, cx| {
        view.set_body(
            Arc::new(Response::new(
                200,
                "OK",
                ResponseBody::File {
                    path: file.path().to_owned(),
                    prefix: Vec::new(),
                },
            )),
            cx,
        )
    });
    cx.run_until_parked();

    let (lines, lossy) = shown(&view, cx);
    assert_eq!(
        lines,
        ["from disk", "second line"],
        "a file-backed body shows the same exact bytes once it is read"
    );
    assert!(!lossy);
}

/// The lines the pane draws and whether it decoded lossily.
fn shown(view: &gpui::Entity<BodyView>, cx: &mut gpui::VisualTestContext) -> (Vec<String>, bool) {
    view.read_with(cx, |view, _| {
        (
            view.lines()
                .iter()
                .map(|line| line.as_str().to_owned())
                .collect(),
            view.is_lossy(),
        )
    })
}
