//! A response can carry the same header name more than once (`Set-Cookie`), and the
//! order the fields arrived in is part of what the user is inspecting (REQ-RESP-05):
//! the table must show every occurrence, in wire order.

use gpui::TestAppContext;
use http_engine::Header;
use ui::test_support::{draw_window, init_test};

use super::HeadersView;

#[gpui::test]
fn duplicate_headers_kept_in_order(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| HeadersView::new(cx));
    let headers = vec![
        Header::new("Set-Cookie", "sid=one"),
        Header::new("Content-Type", "application/json"),
        Header::new("Set-Cookie", "sid=two"),
    ];
    view.update(cx, |view, cx| view.set_headers(&headers, cx));
    draw_window(cx);

    let rows: Vec<(String, String)> = view.read_with(cx, |view, _| {
        view.rows()
            .iter()
            .map(|row| (row.name.to_string(), row.value.to_string()))
            .collect()
    });
    assert_eq!(
        rows,
        [
            ("Set-Cookie".to_owned(), "sid=one".to_owned()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Set-Cookie".to_owned(), "sid=two".to_owned()),
        ],
        "every header field keeps its own row, in the order it arrived"
    );
}
