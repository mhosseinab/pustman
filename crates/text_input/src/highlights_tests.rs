//! The highlight anchors and the multi-line [`TextArea`], driven the way a user drives
//! them: a real window with focus, keystrokes through the keys [`crate::init`] binds,
//! and exact assertions on text, selections and where the highlight spans ended up.
//! These tests are this step's new ones (the earlier modules' tests are read-only), so
//! the editing path both widgets share is proved here from the outside.

use std::ops::Range;

use gpui::{Entity, Focusable, HighlightStyle, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::HighlightSet;
use crate::init;
use crate::text_area::TextArea;

/// A window with one text area holding `text`, drawn and focused, ready for keystrokes.
fn focused_text_area<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextArea>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (area, cx) = cx.add_window_view(|window, cx| {
        let mut area = TextArea::new(window, cx);
        area.set_text(text, cx);
        area
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| area.read_with(cx, |area, cx| area.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (area, cx)
}

/// The text area's text, read through the entity.
fn text_of(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> String {
    cx.read(|cx| area.read_with(cx, |area, _| area.text()))
}

/// The text area's selected byte range — an empty range is the caret.
fn selection_of(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> Range<usize> {
    cx.read(|cx| area.read_with(cx, |area, _| area.selected_range()))
}

/// The byte ranges the text area's highlight spans cover, in the order they were set.
fn highlight_ranges_of(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> Vec<Range<usize>> {
    cx.read(|cx| {
        area.read_with(cx, |area, _| {
            area.highlights()
                .iter()
                .map(|(range, _)| range.clone())
                .collect()
        })
    })
}

/// The byte ranges a [`HighlightSet`] covers, in order.
fn ranges_of(highlights: &HighlightSet) -> Vec<Range<usize>> {
    highlights
        .spans()
        .iter()
        .map(|(range, _)| range.clone())
        .collect()
}

#[gpui::test]
fn highlights_survive_edits_before_them(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx, "hello world");
    cx.update(|_, cx| {
        area.update(cx, |area, cx| {
            area.set_highlights(vec![(6..11, HighlightStyle::default())], cx);
        });
    });

    press_key(cx, "home");
    cx.simulate_input("brave ");

    assert_eq!(text_of(&area, cx), "brave hello world");
    assert_eq!(
        highlight_ranges_of(&area, cx),
        vec![12..17],
        "the span moved by exactly the bytes inserted before it"
    );
}

#[gpui::test]
fn text_area_enter_inserts_newlines_and_line_motion_keeps_the_goal_column(cx: &mut TestAppContext) {
    let (area, cx) = focused_text_area(cx, "");
    cx.simulate_input("hello");
    press_key(cx, "enter");
    cx.simulate_input("hi");
    press_key(cx, "enter");
    cx.simulate_input("tiny");

    assert_eq!(
        text_of(&area, cx),
        "hello\nhi\ntiny",
        "Enter inserts a newline instead of submitting"
    );

    press_key(cx, "up");
    assert_eq!(
        selection_of(&area, cx),
        8..8,
        "up lands at the end of the short line \"hi\""
    );
    press_key(cx, "up");
    assert_eq!(
        selection_of(&area, cx),
        4..4,
        "up keeps the goal column of 4 across the short line"
    );
    press_key(cx, "down");
    assert_eq!(
        selection_of(&area, cx),
        8..8,
        "down clamps to the short line again"
    );
    press_key(cx, "down");
    assert_eq!(
        selection_of(&area, cx),
        13..13,
        "down restores the remembered column"
    );
}

#[test]
fn an_edit_inside_a_highlight_grows_it() {
    let mut highlights = HighlightSet::default();
    highlights.set(vec![(2..4, HighlightStyle::default())], "abcd");

    highlights.shift_for_edit(&(3..3), 2, "abXYcd");

    assert_eq!(
        ranges_of(&highlights),
        vec![2..6],
        "typing inside a span grows it over the inserted text"
    );
}

#[test]
fn an_edit_that_deletes_a_highlight_drops_it() {
    let mut highlights = HighlightSet::default();
    highlights.set(vec![(2..4, HighlightStyle::default())], "abcd");

    highlights.shift_for_edit(&(1..4), 0, "a");

    assert!(
        highlights.spans().is_empty(),
        "a span whose text is deleted is gone"
    );
}

#[test]
fn an_edit_after_a_highlight_leaves_it_alone() {
    let mut highlights = HighlightSet::default();
    highlights.set(vec![(0..2, HighlightStyle::default())], "abcd");

    highlights.shift_for_edit(&(3..3), 1, "abcXd");

    assert_eq!(
        ranges_of(&highlights),
        vec![0..2],
        "an edit after the span never moves it"
    );
}
