//! The find bar follows the formatter override (REQ-RESP-03): the override changes the
//! lines on screen the moment it lands, and the match count must describe those lines —
//! never the ones the previous formatter produced.

use std::sync::Arc;

use gpui::{Entity, SharedString, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::BodyView;
use crate::pretty::Formatter;

#[gpui::test]
fn changing_formatter_override_recounts_find_matches(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the query field takes no typed text
        // (`find_tests.rs`).
        text_input::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));

    // A compact JSON body: Pretty puts a space after every colon and Raw keeps none, so
    // a literal query of a quote, a colon and a space matches only in the Pretty lines.
    let mut response = Response::new(
        200,
        "OK",
        ResponseBody::Bytes(br#"{"name":"one","count":2}"#.to_vec()),
    );
    response
        .headers
        .push(Header::new("Content-Type", "application/json"));
    view.update(cx, |view, cx| view.set_body(Arc::new(response), cx));
    cx.run_until_parked();
    draw_window(cx);

    view.update_in(cx, |view, window, cx| view.open_find(window, cx));
    draw_window(cx);
    cx.simulate_input("\": ");

    let (count, label) = find_status(&view, cx);
    assert_eq!(count, 2, "the colon spaces live only in the Pretty lines");
    assert_eq!(label.as_deref(), Some("1 of 2"));

    // The override lands at once — the Raw lines are on screen until the overridden
    // format is computed — so the count must follow them without waiting for that task.
    view.update(cx, |view, cx| {
        view.set_formatter_override(Some(Formatter::Plain), cx)
    });
    let (count, label) = find_status(&view, cx);
    assert_eq!(
        count, 0,
        "the shown lines are the Raw ones the moment the override lands"
    );
    assert_eq!(label.as_deref(), Some("No matches"));

    // Plain formats to the same text, so the settled state agrees.
    cx.run_until_parked();
    let (count, label) = find_status(&view, cx);
    assert_eq!(count, 0);
    assert_eq!(label.as_deref(), Some("No matches"));
}

/// What the find bar shows: how many matches the query has and what the count label
/// reads.
fn find_status(
    view: &Entity<BodyView>,
    cx: &mut VisualTestContext,
) -> (usize, Option<SharedString>) {
    let bar = view
        .read_with(cx, |view, _| view.find_bar.clone())
        .expect("the find bar is open");
    bar.read_with(cx, |bar, cx| (bar.match_count(), bar.count_label(cx)))
}
