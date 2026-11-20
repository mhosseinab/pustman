//! Safe Preview (REQ-RESP-02): HTML shows as text only — script bodies and markup never
//! reach it — and images render from the bytes the response carried. The app's HTTP
//! client is a recording fake, so anything that fetched — a remote image, a script, a
//! beacon — would show up in its list.

use std::sync::{Arc, Mutex};

use gpui::TestAppContext;
use gpui::http_client::FakeHttpClient;
use http_engine::{Header, Response, ResponseBody};
use image::ImageEncoder as _;
use ui::test_support::{draw_window, init_test};

use super::{PreviewContent, TEXT_ONLY_NOTE};
use crate::body_view::{BodyMode, BodyView};

#[gpui::test]
fn html_preview_does_not_execute_or_fetch(cx: &mut TestAppContext) {
    // Anything the app requests — including anything a preview could be tricked into
    // requesting — lands in this list.
    let requested: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&requested);
    let client = FakeHttpClient::create(move |request| {
        let recorder = Arc::clone(&recorder);
        async move {
            if let Ok(mut seen) = recorder.lock() {
                seen.push(request.uri().to_string());
            }
            Ok(Default::default())
        }
    });
    cx.update(|cx| cx.set_http_client(client));
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));

    let html = br##"<!doctype html>
<html>
<head>
  <title>Ignored page title</title>
  <script>fetch("http://tracker.example/pixel.gif");</script>
  <style>body { display: none; }</style>
</head>
<body>
  <h1>Report</h1>
  <!-- <img src="http://comment.example/c.png"> -->
  <p>Tom &amp; Jerry</p>
  <img src="http://images.example/chart.png" alt="chart">
  <p>Status: 200 &lt;ok&gt;</p>
</body>
</html>"##;
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response("text/html", html)), cx);
        view.set_mode(BodyMode::Preview, cx);
    });
    cx.run_until_parked();
    draw_window(cx);

    let (lines, note) = match preview_of(&view, cx) {
        Some(PreviewContent::Text { lines, note }) => (lines, note),
        other => panic!("an HTML body previews as text, got {other:?}"),
    };
    let shown = lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        shown, "Report\nTom & Jerry\nStatus: 200 <ok>",
        "only the visible text is shown"
    );
    assert_eq!(note, TEXT_ONLY_NOTE);
    for hidden in [
        "<script",
        "fetch(",
        "tracker.example",
        "images.example",
        "comment.example",
        "Ignored page title",
        "display: none",
        "<p>",
    ] {
        assert!(
            !shown.contains(hidden),
            "{hidden:?} must never reach the preview: {shown:?}"
        );
    }
    assert!(
        requested
            .lock()
            .expect("the recorder must not be poisoned")
            .is_empty(),
        "an HTML preview must not request anything"
    );

    // An image renders from the bytes the response carried.
    let png = png_bytes();
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response("image/png", &png)), cx);
    });
    cx.run_until_parked();
    match preview_of(&view, cx) {
        Some(PreviewContent::Image(image)) => {
            assert_eq!(
                image.bytes, png,
                "the image is the body's own bytes, not a download"
            );
        }
        other => panic!("a PNG body previews as an image, got {other:?}"),
    }
    draw_window(cx);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds("preview-image-fallback").is_none(),
        "a decodable image renders its pixels instead of the fallback note"
    );

    // Bytes that will not decode show a note instead of a blank pane — and still
    // nothing was requested anywhere.
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response("image/png", b"not really a png")), cx);
    });
    cx.run_until_parked();
    draw_window(cx);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds("preview-image-fallback").is_some(),
        "an undecodable image shows a note, not a blank pane"
    );
    assert!(
        requested
            .lock()
            .expect("the recorder must not be poisoned")
            .is_empty(),
        "no script ran and nothing was fetched"
    );
}

/// A script is dropped with its content whatever case its tag name is written in:
/// markup names are case-insensitive, and the upper-case spelling is how an evaded
/// sanitizer smuggles the body back into the visible text.
#[test]
fn html_preview_drops_uppercase_script_content() {
    let html = r#"<body><SCRIPT>fetch("http://tracker.example/x")</SCRIPT><p>ok</p></body>"#;
    let shown = match PreviewContent::for_body(Some("text/html"), html.as_bytes()) {
        PreviewContent::Text { lines, .. } => lines
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("an HTML body previews as text, got {other:?}"),
    };
    for hidden in ["fetch", "tracker.example"] {
        assert!(
            !shown.contains(hidden),
            "{hidden:?} must never reach the preview: {shown:?}"
        );
    }
    assert!(shown.contains("ok"), "the visible text stays: {shown:?}");
}

/// What the pane shows for the body on screen.
fn preview_of(
    view: &gpui::Entity<BodyView>,
    cx: &mut gpui::VisualTestContext,
) -> Option<PreviewContent> {
    view.read_with(cx, |view, _| view.preview_content().cloned())
}

/// A response whose body is `body`, labelled `content_type`.
fn response(content_type: &str, body: &[u8]) -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.to_vec()));
    response
        .headers
        .push(Header::new("Content-Type", content_type));
    response
}

/// A real one-pixel PNG, so the image path decodes something instead of guessing.
fn png_bytes() -> Vec<u8> {
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .expect("the test image must encode");
    encoded
}
