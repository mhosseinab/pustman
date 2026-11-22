//! A streamed body's size is measured by the read that loads it (#122): the Size metric
//! precomputes as the unmeasured em dash when the response arrives —
//! [`crate::status_line::body_size`] cannot know a file-backed body's length without
//! touching the disk — and must show the measured byte count once the load lands.

use std::io::Write as _;

use gpui::{TestAppContext, VisualTestContext};
use http_engine::{Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use crate::ResponseView;
use crate::status_line::status_meta_selector;

/// What the metric shows while the body is unmeasured (ADR-0003).
const UNMEASURED: &str = "—";

#[gpui::test]
fn file_body_size_shows_the_measured_length_once_the_body_lands(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));

    // A body streamed to disk: its length is known only to the read that loads it.
    let body = b"measured on disk";
    let mut file = tempfile::NamedTempFile::new().expect("the temp file must be created");
    file.write_all(body)
        .expect("the temp file must write the body");
    view.update(cx, |view, cx| {
        view.set_response(
            Response::new(
                200,
                "OK",
                ResponseBody::File {
                    path: file.path().to_owned(),
                    prefix: Vec::new(),
                },
            ),
            cx,
        )
    });
    draw_window(cx);
    assert!(
        shown(cx, status_meta_selector("Size", UNMEASURED)),
        "before the read lands, the size is honestly unmeasured"
    );

    cx.run_until_parked();
    draw_window(cx);

    assert!(
        shown(cx, status_meta_selector("Size", "16 B")),
        "the measured length feeds back into the Size metric"
    );
    assert!(
        !shown(cx, status_meta_selector("Size", UNMEASURED)),
        "the em dash is gone once the body is measured"
    );
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}
