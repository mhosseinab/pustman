//! The boundary tie-break [`HighlightSet::shift_for_edit`] resolves by which end of a
//! span an insertion lands on: at a span's start the span moves with the text so the
//! typed bytes stay unhighlighted, at its end the span is left as it is.

use std::ops::Range;

use gpui::{Font, HighlightStyle, Hsla, TextRun};

use super::HighlightSet;

/// The byte ranges a [`HighlightSet`] covers, in order.
fn ranges_of(highlights: &HighlightSet) -> Vec<Range<usize>> {
    highlights
        .spans()
        .iter()
        .map(|(range, _)| range.clone())
        .collect()
}

/// A text run of `len` bytes in `color`, the base every span paints over.
fn base_run(len: usize, color: Hsla) -> TextRun {
    TextRun {
        len,
        font: Font::default(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

#[test]
fn typing_at_a_span_start_moves_the_span_without_highlighting_the_new_text() {
    let base_color = Hsla {
        h: 0.,
        s: 0.,
        l: 0.,
        a: 1.,
    };
    let highlight_color = Hsla {
        h: 0.,
        s: 1.,
        l: 0.5,
        a: 1.,
    };
    let mut highlights = HighlightSet::default();
    highlights.set(
        vec![(
            3..5,
            HighlightStyle {
                color: Some(highlight_color),
                ..HighlightStyle::default()
            },
        )],
        "abcdef",
    );

    // "cd" is highlighted on "abcdef"; typing "XX" at the span's start.
    highlights.shift_for_edit(&(3..3), 2, "abXXcdef");

    assert_eq!(
        ranges_of(&highlights),
        vec![5..7],
        "the span moved over the typed text instead of growing around it"
    );
    let painted: Vec<(usize, Hsla)> = highlights
        .runs(&base_run(8, base_color), None)
        .iter()
        .map(|run| (run.len, run.color))
        .collect();
    assert_eq!(
        painted,
        vec![(5, base_color), (2, highlight_color), (1, base_color)],
        "only the moved span is highlighted; the typed text keeps the base color"
    );

    // The mirror tie-break this fix must not break: an insertion at a span's end.
    let mut highlights = HighlightSet::default();
    highlights.set(vec![(3..5, HighlightStyle::default())], "abcdef");

    highlights.shift_for_edit(&(5..5), 2, "abcdeXXf");

    assert_eq!(
        ranges_of(&highlights),
        vec![3..5],
        "an insertion at a span's end leaves the span as it is"
    );
}
