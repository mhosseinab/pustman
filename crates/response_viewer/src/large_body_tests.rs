//! The large-body fallback (REQ-RESP-06): a body over `request.response_pretty_limit_mb`
//! opens Raw with a banner that can reveal the temp file behind it, and a body under the
//! limit changes nothing.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::TestAppContext;
use http_engine::{Response, ResponseBody, SpillWriter};
use settings::SettingsStore;
use ui::test_support::init_test;

use super::exceeds_pretty_limit;
use crate::BodyView;
use crate::body_view::BodyMode;

/// One megabyte, the unit `request.response_pretty_limit_mb` counts in.
const MEGABYTE: u64 = 1024 * 1024;

#[gpui::test]
fn large_body_falls_back_to_raw_with_banner(cx: &mut TestAppContext) {
    // The limit is the one the user sets in `request.response_pretty_limit_mb`, read
    // through the settings store the app installs.
    cx.update(init_test);
    cx.update(|cx| {
        let applied =
            SettingsStore::reload(cx, Some(r#"{"request":{"response_pretty_limit_mb":1}}"#));
        assert_eq!(applied, Ok(()), "the pretty limit is a user-set setting");
    });

    // A body over the limit that spilled to a temp file: Raw shows with the banner, and
    // the banner's reveal target is that file.
    let (body, spilled_path) = spilled_body(2 * MEGABYTE);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(Response::new(200, "OK", body)), cx)
    });

    let notice = view
        .read_with(cx, |view, _| {
            assert_eq!(
                view.mode(),
                BodyMode::Raw,
                "a body over the limit opens Raw"
            );
            view.large_body().cloned()
        })
        .expect("the banner is up while the body is over the limit");
    assert_eq!(
        notice.file_path(),
        Some(spilled_path.as_path()),
        "the banner's reveal target is the spilled temp file"
    );
    assert!(
        notice.message().contains("larger than 1 MB"),
        "the banner names the limit the body went over"
    );

    // Pretty stays one pick away (the fallback locks no mode), and a body under the
    // limit then arrives untouched: no banner and no mode change.
    view.update(cx, |view, cx| view.set_mode(BodyMode::Pretty, cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.set_body(
            Arc::new(Response::new(
                200,
                "OK",
                ResponseBody::Bytes(b"{\"small\": true}".to_vec()),
            )),
            cx,
        )
    });
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.mode(),
            BodyMode::Pretty,
            "a body under the limit leaves the mode the user picked"
        );
        assert!(
            view.large_body().is_none(),
            "a body under the limit shows no banner"
        );
    });
}

/// The pure boundary check: the limit is inclusive — a body at exactly the limit is
/// still pretty-printed (REQ-RESP-06 says *above* the limit falls back).
#[test]
fn body_over_the_pretty_limit_is_flagged_and_one_at_the_limit_is_not() {
    assert!(
        exceeds_pretty_limit(2 * MEGABYTE, 1),
        "a body past the limit is flagged"
    );
    assert!(
        !exceeds_pretty_limit(MEGABYTE, 1),
        "a body at the limit itself is still pretty-printed"
    );
    assert!(
        !exceeds_pretty_limit(MEGABYTE - 1, 1),
        "a body under the limit is not flagged"
    );
}

/// A body of `total` bytes whose tail spilled past a 1 KB in-memory prefix into its
/// temp file — what a send over the spill limit hands the pane (REQ-RESP-06) — plus the
/// path of that file. `SpillWriter` is the only way a `SpilledFile` comes into
/// existence, and its pushes are tokio futures, so the test drives them on a tokio
/// runtime of its own.
fn spilled_body(total: u64) -> (ResponseBody, PathBuf) {
    let total = usize::try_from(total).expect("the test's body size fits usize");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test's tokio runtime must build");
    let mut writer = SpillWriter::new(1024);
    let chunk = vec![b'x'; 64 * 1024];
    let mut written = 0;
    while written < total {
        let take = chunk.len().min(total - written);
        runtime
            .block_on(writer.push(&chunk[..take]))
            .expect("the body must spill to its temp file");
        written += take;
    }
    let body = runtime
        .block_on(writer.into_body())
        .expect("the spilled body must come back");
    let ResponseBody::Spilled { file, .. } = &body else {
        panic!("a body past the spill limit must come back spilled");
    };
    let spilled_path = file.path().to_owned();
    (body, spilled_path)
}
