//! The highlight spans a text widget paints over its text: UTF-8 byte ranges with a
//! [`HighlightStyle`], anchored to the text so an edit made before a span moves it with
//! the text instead of leaving it pointing at the wrong words.
//!
//! `Decision:` span boundaries are anchors mapped through each edit: an edit strictly
//! before a span shifts it by the inserted-minus-deleted length, an edit strictly after
//! it leaves it alone, and an edit that reaches inside a span grows or shrinks it over
//! the changed bytes — typing inside highlighted text keeps it highlighted. A span whose
//! text is deleted entirely collapses and is dropped: a highlight over nothing has
//! nothing to point at.
//!
//! `Decision:` spans may overlap and are painted in the order they were given, later
//! spans winning field by field over earlier ones. [`HighlightStyle::fade_out`] is not
//! painted — a per-run opacity has no [`TextRun`] equivalent — and is ignored.

use std::ops::Range;

use gpui::{HighlightStyle, TextRun, UnderlineStyle};

/// One highlighted span: the UTF-8 byte range it covers and how it paints.
pub type Highlight = (Range<usize>, HighlightStyle);

/// The highlight spans on one widget's text, in the order the caller set them.
#[derive(Default)]
pub struct HighlightSet {
    spans: Vec<Highlight>,
}

impl HighlightSet {
    /// An empty set: nothing is highlighted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the spans with `highlights`, clamping each to `text`'s char boundaries
    /// and dropping the ones that end up empty. A widget calls this from
    /// `set_highlights`, with the text the spans describe.
    pub fn set(&mut self, highlights: Vec<Highlight>, text: &str) {
        self.spans = highlights
            .into_iter()
            .filter_map(|(range, style)| {
                let range = clamp(range, text);
                (range.start < range.end).then_some((range, style))
            })
            .collect();
    }

    /// The spans in the order they were set.
    pub fn spans(&self) -> &[Highlight] {
        &self.spans
    }

    /// Drop every span: the text they described is gone.
    pub fn clear(&mut self) {
        self.spans.clear();
    }

    /// Move the spans through the edit that replaced `replaced` with `inserted_len`
    /// bytes, on the text as it stands *after* the edit (which also anchors the
    /// clamping to char boundaries).
    ///
    /// Both ends map through the same function, the boundary cases tie-broken by which
    /// end it is: an edit ending exactly where a span starts is before it and the span
    /// moves — an insertion at a span's start never highlights the typed text — while
    /// an edit starting exactly where a span ends is after it and the span stays.
    pub fn shift_for_edit(&mut self, replaced: &Range<usize>, inserted_len: usize, text: &str) {
        let deleted_len = replaced.end.saturating_sub(replaced.start);
        let delta = inserted_len as i64 - deleted_len as i64;
        self.spans.retain_mut(|(range, _)| {
            let shifted = mapped(range.start, Boundary::Start, replaced, inserted_len, delta)
                ..mapped(range.end, Boundary::End, replaced, inserted_len, delta);
            *range = clamp(shifted, text);
            range.start < range.end
        });
    }

    /// The [`TextRun`]s that paint `base`'s text: `base` under every span covering a
    /// piece of it — later spans win — plus the IME composition's `marked` underline.
    /// The runs cover `base.len` bytes exactly, which is what the text shaper expects.
    pub fn runs(
        &self,
        base: &TextRun,
        marked: Option<(Range<usize>, UnderlineStyle)>,
    ) -> Vec<TextRun> {
        let mut boundaries = vec![0, base.len];
        for (range, _) in &self.spans {
            boundaries.push(range.start);
            boundaries.push(range.end);
        }
        if let Some((range, _)) = &marked {
            boundaries.push(range.start);
            boundaries.push(range.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        let mut runs = Vec::with_capacity(boundaries.len());
        for pair in boundaries.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let mut run = base.clone();
            run.len = end - start;
            for (range, style) in &self.spans {
                if range.start <= start && end <= range.end {
                    apply(&mut run, style);
                }
            }
            if let Some((range, underline)) = &marked
                && range.start <= start
                && end <= range.end
            {
                run.underline = Some(*underline);
            }
            runs.push(run);
        }
        runs
    }
}

/// Which end of a span an offset anchors. The two ends tie-break a zero-length edit
/// at the offset differently — see [`mapped`].
#[derive(Clone, Copy)]
enum Boundary {
    /// The span's start.
    Start,
    /// The span's end.
    End,
}

/// Where `offset` lands after an edit that replaced `replaced` with `inserted_len`
/// bytes: unchanged before the edit, shifted by the length delta after it, and pulled
/// to the edit point when the edit deleted it.
///
/// A zero-length edit exactly at `offset` is both an edit ending at a span's start and
/// one starting at a span's end, so `boundary` breaks the tie: the start moves with the
/// inserted text and the end stays.
fn mapped(
    offset: usize,
    boundary: Boundary,
    replaced: &Range<usize>,
    inserted_len: usize,
    delta: i64,
) -> usize {
    if offset == replaced.start && replaced.start == replaced.end {
        return match boundary {
            Boundary::Start => (offset as i64 + delta).max(0) as usize,
            Boundary::End => offset,
        };
    }
    if offset <= replaced.start {
        offset
    } else if offset >= replaced.end {
        (offset as i64 + delta).max(0) as usize
    } else {
        replaced.start + inserted_len
    }
}

/// Paint `style` over `run` — only the fields the two types share.
fn apply(run: &mut TextRun, style: &HighlightStyle) {
    if let Some(color) = style.color {
        run.color = color;
    }
    if let Some(background_color) = style.background_color {
        run.background_color = Some(background_color);
    }
    if let Some(underline) = style.underline {
        run.underline = Some(underline);
    }
    if let Some(strikethrough) = style.strikethrough {
        run.strikethrough = Some(strikethrough);
    }
    if let Some(font_weight) = style.font_weight {
        run.font.weight = font_weight;
    }
    if let Some(font_style) = style.font_style {
        run.font.style = font_style;
    }
}

/// The range clamped to `text` and snapped outward to char boundaries, so a shifted
/// span can never point into the middle of a character.
fn clamp(range: Range<usize>, text: &str) -> Range<usize> {
    let start = snap_left(range.start.min(text.len()), text);
    let end = snap_right(range.end.min(text.len()), text).max(start);
    start..end
}

/// The nearest char boundary at or before `offset`.
fn snap_left(offset: usize, text: &str) -> usize {
    let mut offset = offset;
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// The nearest char boundary at or after `offset`.
fn snap_right(offset: usize, text: &str) -> usize {
    let mut offset = offset;
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

#[cfg(test)]
#[path = "highlights_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "highlights_insert_tests.rs"]
mod insert_tests;
