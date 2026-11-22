//! The find bar's error state (#161): a query that does not compile shows its inline
//! error and nothing else. The count must stay silent there — "No matches" beside a
//! compile error contradicts it, because such a query has no matches only by having no
//! meaning.

use std::sync::Arc;

use gpui::{Entity, Focusable, SharedString, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::FindBar;

#[gpui::test]
fn find_regex_error_shows_only_the_error_not_no_matches(cx: &mut TestAppContext) {
    let (bar, cx) = open_find_bar(cx);
    set_lines(&bar, cx, &["alpha", "beta"]);
    set_regex(&bar, cx, true);
    type_query(&bar, cx, "(");

    let (label, shows_error) = status(&bar, cx);
    assert!(
        shows_error,
        "an uncompilable pattern shows its compile error inline"
    );
    assert_eq!(
        label, None,
        "the count says nothing while the error speaks — no 'No matches' beside it"
    );
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

/// The regex option, through the path the toggle takes.
fn set_regex(bar: &Entity<FindBar>, cx: &mut VisualTestContext, regex: bool) {
    bar.update(cx, |bar, cx| bar.set_regex(regex, cx));
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

/// What the bar shows the user: the count beside the controls and the inline error under
/// the field.
fn status(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> (Option<SharedString>, bool) {
    bar.read_with(cx, |bar, cx| (bar.count_label(cx), bar.error.is_some()))
}
