//! Pretty mode (REQ-RESP-02): the formatter is auto-detected from `Content-Type` and can
//! be overridden by hand — a compact JSON body renders indented and multi-line, and a
//! body its `Content-Type` mislabels stays unformatted until the override picks the right
//! formatter.

use std::sync::Arc;

use gpui::TestAppContext;
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::Formatter;
use crate::body_view::{BodyMode, BodyView};

#[gpui::test]
fn json_body_pretty_printed(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));

    let body = br#"{"name":"pustman","tags":["a","b"]}"#;
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.to_vec()));
    response
        .headers
        .push(Header::new("Content-Type", "application/json"));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response), cx);
        view.set_mode(BodyMode::Pretty, cx);
    });
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        shown(&view, cx),
        [
            "{",
            "  \"name\": \"pustman\",",
            "  \"tags\": [",
            "    \"a\",",
            "    \"b\"",
            "  ]",
            "}"
        ],
        "a compact JSON body renders indented, one value per line, in Pretty mode"
    );
}

#[gpui::test]
fn content_type_override_switches_formatter(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));

    // An XML body the server labelled text/plain: detection has nothing to go on, so
    // Pretty shows the text unformatted.
    let body = b"<note><to>Tove</to></note>";
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.to_vec()));
    response
        .headers
        .push(Header::new("Content-Type", "text/plain"));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response), cx);
        view.set_mode(BodyMode::Pretty, cx);
    });
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        shown(&view, cx),
        ["<note><to>Tove</to></note>"],
        "a mislabelled body stays unformatted while detection says Plain"
    );

    // The override picks XML by hand: the same body now renders indented.
    view.update(cx, |view, cx| {
        view.set_formatter_override(Some(Formatter::Xml), cx)
    });
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        shown(&view, cx),
        ["<note>", "  <to>", "    Tove", "  </to>", "</note>"],
        "overriding the formatter to XML indents the same body by tag depth"
    );
}

/// The lines the pane draws, as strings.
fn shown(view: &gpui::Entity<BodyView>, cx: &mut gpui::VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.shown_lines()
            .iter()
            .map(|line| line.as_str().to_owned())
            .collect()
    })
}

/// A body of deeply nested, unclosed tags must not indent quadratically: the formatted
/// output stays bounded by the input, or the formatter refuses — and the view falls back
/// to the Raw text with its inline note. Response bodies are untrusted data and may carry
/// any shape.
#[test]
fn deeply_nested_unclosed_markup_stays_bounded() {
    let body = "<a>".repeat(30_000);
    // An error (the output budget refusing the format) is bounded too: the pane then
    // shows the Raw text with its inline note.
    if let Ok(formatted) = Formatter::Xml.format(body.as_bytes()) {
        assert!(
            formatted.len() <= body.len() * 4,
            "a {}-byte body of unclosed tags formatted to {} bytes; the output must stay \
             linear in the input",
            body.len(),
            formatted.len(),
        );
    }
}
