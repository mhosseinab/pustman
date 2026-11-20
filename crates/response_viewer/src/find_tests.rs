//! Find in the response body (REQ-RESP-03): what counts as a match. Both tests type
//! into the query field and toggle the options the way a user does, and assert the count
//! the controls show.

use std::sync::Arc;

use gpui::{Entity, Focusable, SharedString, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::FindBar;

#[gpui::test]
fn find_regex_counts_matches(cx: &mut TestAppContext) {
    let (bar, cx) = open_find_bar(cx);
    set_lines(
        &bar,
        cx,
        &["user101 and 202", "total 3030", "no digits here"],
    );
    set_regex(&bar, cx, true);
    type_query(&bar, cx, r"\d+");

    let (count, label) = status(&bar, cx);
    assert_eq!(
        count, 3,
        "the pattern matches every run of digits, on any line"
    );
    assert_eq!(
        label.as_deref(),
        Some("1 of 3"),
        "the count reads 'current of total'"
    );

    set_regex(&bar, cx, false);
    let (count, label) = status(&bar, cx);
    assert_eq!(
        count, 0,
        "without the regex option the query is literal text"
    );
    assert_eq!(
        label.as_deref(),
        Some("No matches"),
        "a search with nothing behind it says so"
    );
}

#[gpui::test]
fn find_case_sensitive_toggle(cx: &mut TestAppContext) {
    let (bar, cx) = open_find_bar(cx);
    set_lines(&bar, cx, &["Error: none", "error count 2", "ERROR state"]);
    type_query(&bar, cx, "error");

    let (count, _) = status(&bar, cx);
    assert_eq!(
        count, 3,
        "search is case-insensitive until the case toggle is on"
    );

    set_case_sensitive(&bar, cx, true);
    let (count, label) = status(&bar, cx);
    assert_eq!(count, 1, "the case toggle keeps only the exact-case match");
    assert_eq!(label.as_deref(), Some("1 of 1"));
}

/// Open a find bar the way the body does, under the app's real init order.
fn open_find_bar(cx: &mut TestAppContext) -> (Entity<FindBar>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the editing keys are unbound and typing into
        // the query field does nothing.
        text_input::init(cx);
    });
    cx.add_window_view(FindBar::new)
}

/// Give the bar these body lines to search.
fn set_lines(bar: &Entity<FindBar>, cx: &mut VisualTestContext, lines: &[&str]) {
    let lines = Arc::new(
        lines
            .iter()
            .map(|line| SharedString::from(*line))
            .collect::<Vec<_>>(),
    );
    bar.update(cx, |bar, cx| bar.set_lines(lines, cx));
    cx.run_until_parked();
}

/// Focus the query field and type into it, the way a user does.
fn type_query(bar: &Entity<FindBar>, cx: &mut VisualTestContext, query: &str) {
    let focus_handle = bar.read_with(cx, |bar, cx| bar.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    cx.simulate_input(query);
    cx.run_until_parked();
}

/// The regex option, through the path the toggle takes.
fn set_regex(bar: &Entity<FindBar>, cx: &mut VisualTestContext, regex: bool) {
    bar.update(cx, |bar, cx| bar.set_regex(regex, cx));
    cx.run_until_parked();
}

/// The case-sensitivity option, through the path the toggle takes.
fn set_case_sensitive(bar: &Entity<FindBar>, cx: &mut VisualTestContext, case_sensitive: bool) {
    bar.update(cx, |bar, cx| bar.set_case_sensitive(case_sensitive, cx));
    cx.run_until_parked();
}

/// What the count beside the controls says.
fn status(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> (usize, Option<SharedString>) {
    bar.read_with(cx, |bar, cx| (bar.match_count(), bar.count_label(cx)))
}
