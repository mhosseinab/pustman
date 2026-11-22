//! The find bar's own keys (#162): Escape closes the bar and hands focus back to the
//! body, Enter steps to the next match, Shift-Enter to the previous one — the three
//! keyboard paths find.rs's `Decision:` describes but never tested. The keys are
//! pressed for real: Enter travels through the query field's `Submitted` event, Escape
//! and Shift-Enter through the bar's key listener.

use std::sync::Arc;

use gpui::{Entity, Focusable, SharedString, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::FindBar;
use crate::BodyView;

/// Three digit runs over three lines: a query of `\d+` in regex mode has three matches
/// to step through, one per line.
const LINES: [&str; 3] = ["user101 and 202", "total 3030", "no digits here"];

#[gpui::test]
fn find_enter_steps_to_the_next_match_and_wraps(cx: &mut TestAppContext) {
    let (bar, cx) = open_find_bar(cx);
    set_lines(&bar, cx, &LINES);
    set_regex(&bar, cx, true);
    type_query(&bar, cx, r"\d+");
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("1 of 3"),
        "the query lands"
    );

    press_key(cx, "enter");
    cx.run_until_parked();
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("2 of 3"),
        "Enter walks to the next match"
    );

    press_key(cx, "enter");
    press_key(cx, "enter");
    cx.run_until_parked();
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("1 of 3"),
        "and wraps past the last match back to the first"
    );
}

#[gpui::test]
fn find_shift_enter_steps_to_the_previous_match_and_wraps(cx: &mut TestAppContext) {
    let (bar, cx) = open_find_bar(cx);
    set_lines(&bar, cx, &LINES);
    set_regex(&bar, cx, true);
    type_query(&bar, cx, r"\d+");
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("1 of 3"),
        "the query lands"
    );

    press_key(cx, "shift-enter");
    cx.run_until_parked();
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("3 of 3"),
        "Shift-Enter walks to the previous match, wrapping before the first"
    );

    press_key(cx, "shift-enter");
    cx.run_until_parked();
    assert_eq!(
        label(&bar, cx).as_deref(),
        Some("2 of 3"),
        "and steps back one match at a time from there"
    );
}

#[gpui::test]
fn find_escape_closes_the_bar_and_refocuses_the_body(cx: &mut TestAppContext) {
    let (view, cx) = body_view(cx);
    view.update_in(cx, |view, window, cx| view.open_find(window, cx));
    draw_window(cx);
    cx.simulate_input("digits");
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.find_bar().is_some()),
        "the find bar is open over the body"
    );

    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        view.read_with(cx, |view, _| view.find_bar().is_none()),
        "Escape closes the find bar"
    );
    let body_focus = view.read_with(cx, |view, cx| view.focus_handle(cx));
    let focused = cx.update(|window, _cx| body_focus.is_focused(window));
    assert!(focused, "Escape hands focus back to the body");
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

/// The body pane the Escape test opens the bar on.
fn body_view(cx: &mut TestAppContext) -> (Entity<BodyView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the query field takes no typed text.
        text_input::init(cx);
    });
    cx.add_window_view(|_, cx| BodyView::new(cx))
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

/// What the count beside the controls says — where the current match sits.
fn label(bar: &Entity<FindBar>, cx: &mut VisualTestContext) -> Option<SharedString> {
    bar.read_with(cx, |bar, cx| bar.count_label(cx))
}
