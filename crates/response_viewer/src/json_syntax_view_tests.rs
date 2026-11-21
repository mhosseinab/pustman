//! Pretty mode colours a JSON body: the spans are computed with the formatted lines and
//! cached beside them, and a body Pretty does not format as JSON gets none.

use std::sync::Arc;

use gpui::{Entity, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use crate::body_view::{BodyMode, BodyView};
use crate::json_syntax::JsonToken;

fn pretty_body<'a>(
    cx: &'a mut TestAppContext,
    content_type: &str,
    body: &[u8],
) -> (Entity<BodyView>, &'a mut VisualTestContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.to_vec()));
    response
        .headers
        .push(Header::new("Content-Type", content_type));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response), cx);
        view.set_mode(BodyMode::Pretty, cx);
    });
    cx.run_until_parked();
    draw_window(cx);
    (view, cx)
}

#[gpui::test]
fn test_a_pretty_json_body_carries_a_span_list_per_line(cx: &mut TestAppContext) {
    let (view, cx) = pretty_body(cx, "application/json", br#"{"name":"pustman","n":1}"#);

    view.read_with(cx, |view, _| {
        let spans = view.syntax_spans();
        assert_eq!(spans.len(), 4, "one span list per formatted line");
        let tokens = spans
            .get(1)
            .map(|line| line.iter().map(|(_, token)| *token).collect::<Vec<_>>());
        assert_eq!(
            tokens,
            Some(vec![
                JsonToken::Key,
                JsonToken::Punctuation,
                JsonToken::String,
                JsonToken::Punctuation
            ]),
            "the `\"name\": \"pustman\",` line is a key, a colon, a string and a comma"
        );
    });
}

#[gpui::test]
fn test_a_body_not_formatted_as_json_is_not_coloured(cx: &mut TestAppContext) {
    let (view, cx) = pretty_body(cx, "text/plain", br#"{"name":"pustman"}"#);

    view.read_with(cx, |view, _| {
        assert!(
            view.syntax_spans().is_empty(),
            "plain text is drawn uncoloured even when it looks like JSON"
        );
    });
}
