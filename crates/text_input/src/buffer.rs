//! The editing substrate of [`crate::input::TextInput`]: a gap buffer behind the small
//! surface a text field needs — read the text, replace a byte range, and find the
//! grapheme and word boundaries the cursor moves to.
//!
//! `Decision:` a gap buffer, not a `String` and not a rope. A text field edits at one
//! point — the cursor — and a gap buffer makes that edit the cost of the inserted bytes
//! without copying the rest of the text per keystroke; `String::replace` would reallocate
//! and copy the tail on every keystroke, while a rope pays for random access a
//! single-line field never asks for and would need a new dependency. The gap refills in
//! blocks of at least [`GAP_REFILL`] bytes and at least the text length, so the
//! occasional refill is amortized over the keystrokes it serves. Boundary scans use
//! `unicode-segmentation` (grapheme clusters and UAX #29 words) — never hand-rolled
//! segmentation — over a *window* of the logical text: the two segments are joined only
//! when the gap falls inside the window (see [`Buffer::window`]), so cursor motion at the
//! gap — the common case — never allocates.

use std::borrow::Cow;
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// The smallest gap a refill buys. A refill also always buys at least the text length,
/// so the O(text) cost of refilling is spread over at least that many keystrokes.
const GAP_REFILL: usize = 256;

/// A text buffer that edits in place around a gap.
///
/// Text lives in one `Vec<u8>` with an invisible gap: the bytes before [`Self::gap_start`]
/// are the text before the cursor point, the bytes after [`Self::gap_end`] are the text
/// after it. Offsets in the public API are logical UTF-8 byte offsets into the text —
/// the gap is invisible — and every range is clamped to the text and snapped outward to
/// char boundaries, so an out-of-range or mid-character edit can never panic.
pub struct Buffer {
    storage: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
    revision: u64,
}

impl Buffer {
    /// An empty buffer.
    pub fn new() -> Self {
        Self {
            storage: Vec::new(),
            gap_start: 0,
            gap_end: 0,
            revision: 0,
        }
    }

    /// A buffer holding `text`, with the gap at the end: a cursor placed at the end —
    /// where [`crate::input::TextInput`] starts one — types without moving the gap.
    pub fn from_text(text: &str) -> Self {
        let end = text.len();
        Self {
            storage: text.as_bytes().to_vec(),
            gap_start: end,
            gap_end: end,
            revision: 0,
        }
    }

    /// Bytes of text.
    pub fn len(&self) -> usize {
        self.storage.len() - (self.gap_end - self.gap_start)
    }

    /// Whether there is no text at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A counter bumped by every edit. A layout cache records it to know whether the
    /// text it was shaped from is still the current text.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The whole text. Borrowed when the gap sits at an end of the storage — no copy —
    /// and joined into one `String` only when the gap is inside the text.
    pub fn text(&self) -> Cow<'_, str> {
        self.window(0..self.len())
    }

    /// The text in `range`, clamped and snapped like [`Self::replace`].
    pub fn slice(&self, range: Range<usize>) -> String {
        self.window(range).into_owned()
    }

    /// The text's characters across the gap, in order and without copying.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.prefix().chars().chain(self.suffix().chars())
    }

    /// Replace the logical `range` with `text`.
    ///
    /// The range is clamped to the text and snapped outward to char boundaries, so a
    /// caller cannot split a character or index past the end. Editing at the gap — where
    /// the cursor is — costs the inserted bytes and nothing else; the gap moves only when
    /// the edit is somewhere else.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        let range = self.clamp_range(range);
        self.move_gap_to(range.start);
        // With the gap at the edit point, the deleted bytes are the head of the
        // suffix: dropping them is moving one gap edge.
        self.gap_end = (self.gap_end + range.end - range.start).min(self.storage.len());
        if text.len() > self.gap_end - self.gap_start {
            self.grow_gap(text.len());
        }
        self.storage[self.gap_start..self.gap_start + text.len()].copy_from_slice(text.as_bytes());
        self.gap_start += text.len();
        self.revision = self.revision.wrapping_add(1);
    }

    /// The start of the grapheme cluster before `offset` — one left-arrow press — or 0.
    pub fn previous_boundary(&self, offset: usize) -> usize {
        let offset = self.snap_left(offset);
        if offset == 0 {
            return 0;
        }
        self.window(0..offset)
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    /// The end of the grapheme cluster at or after `offset` — one right-arrow press — or
    /// the text length.
    pub fn next_boundary(&self, offset: usize) -> usize {
        let offset = self.snap_left(offset);
        self.window(offset..self.len())
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > 0).then_some(offset + index))
            .unwrap_or(self.len())
    }

    /// The start of the last word beginning before `offset` — one word-left press — or 0.
    /// Punctuation and whitespace are not words (UAX #29 `unicode_word_indices`), so the
    /// cursor lands on the start of `world` in `hello, world`, not on the comma.
    pub fn previous_word_start(&self, offset: usize) -> usize {
        let offset = self.snap_left(offset);
        if offset == 0 {
            return 0;
        }
        self.window(0..offset)
            .unicode_word_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    /// The end of the first word at or after `offset` — one word-right press — or the
    /// text length. From inside a word this is that word's end.
    pub fn next_word_end(&self, offset: usize) -> usize {
        let offset = self.snap_left(offset);
        self.window(offset..self.len())
            .unicode_word_indices()
            .next()
            .map(|(index, word)| offset + index + word.len())
            .unwrap_or(self.len())
    }

    /// The logical bytes in `range` as one `str` whose first byte is `range.start`.
    ///
    /// A scan of one side of the gap (cursor motion at the gap, the common case) borrows
    /// one segment and allocates nothing; a window that crosses the gap joins only the
    /// two parts it covers. The window starts at a char boundary by construction, which
    /// is what makes the segmentation scans exact: a scan from the text start or a
    /// segment edge sees whole characters.
    fn window(&self, range: Range<usize>) -> Cow<'_, str> {
        let range = self.clamp_range(range);
        let gap = self.gap_end - self.gap_start;
        if range.end <= self.gap_start {
            Cow::Borrowed(segment(&self.storage[range.start..range.end]))
        } else if range.start >= self.gap_start {
            Cow::Borrowed(segment(&self.storage[range.start + gap..range.end + gap]))
        } else {
            let mut text = String::with_capacity(range.end - range.start);
            text.push_str(segment(&self.storage[range.start..self.gap_start]));
            text.push_str(segment(&self.storage[self.gap_end..range.end + gap]));
            Cow::Owned(text)
        }
    }

    /// The text before the gap.
    fn prefix(&self) -> &str {
        segment(&self.storage[..self.gap_start])
    }

    /// The text after the gap.
    fn suffix(&self) -> &str {
        segment(&self.storage[self.gap_end..])
    }

    /// A range clamped to the text and snapped outward to char boundaries, so slicing
    /// the segments at it can never panic and never splits a character.
    fn clamp_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.snap_left(range.start);
        let end = self.snap_right(range.end).max(start);
        start..end
    }

    /// The nearest char boundary at or before `offset`.
    fn snap_left(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.len());
        while offset > 0 && !self.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// The nearest char boundary at or after `offset`.
    fn snap_right(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.len());
        while offset < self.len() && !self.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }

    /// Whether a logical offset falls between two characters. The check runs on the
    /// segment the offset is in; both edges of the gap are char boundaries because every
    /// write copies `str` bytes in whole characters.
    fn is_char_boundary(&self, offset: usize) -> bool {
        let offset = offset.min(self.len());
        if offset < self.gap_start {
            self.prefix().is_char_boundary(offset)
        } else {
            self.suffix().is_char_boundary(offset - self.gap_start)
        }
    }

    /// Move the gap so it starts at the logical `offset`, shifting bytes inside the
    /// storage. The gap never leaves the text; no reallocation.
    fn move_gap_to(&mut self, offset: usize) {
        let offset = offset.min(self.len());
        if offset < self.gap_start {
            let moved = self.gap_start - offset;
            let destination = self.gap_end - moved;
            self.storage
                .copy_within(offset..self.gap_start, destination);
            self.gap_start = offset;
            self.gap_end = destination;
        } else if offset > self.gap_start {
            let moved = offset - self.gap_start;
            self.storage
                .copy_within(self.gap_end..self.gap_end + moved, self.gap_start);
            self.gap_start = offset;
            self.gap_end += moved;
        }
    }

    /// Enlarge the gap to at least `needed` bytes. The new gap is at least the text
    /// length, so the copy this costs is paid back over at least that many keystrokes —
    /// typing never copies the whole text per keystroke.
    fn grow_gap(&mut self, needed: usize) {
        let gap_size = needed.max(self.len()).max(GAP_REFILL);
        let mut grown = Vec::with_capacity(self.len() + gap_size);
        grown.extend_from_slice(&self.storage[..self.gap_start]);
        let suffix_start = grown.len();
        grown.resize(suffix_start + gap_size, 0);
        grown.extend_from_slice(&self.storage[self.gap_end..]);
        self.storage = grown;
        self.gap_end = suffix_start + gap_size;
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

/// The bytes as text. Every write copies `str` bytes in whole characters, so both
/// segments are valid UTF-8; the checked conversion keeps that a fact without `unsafe`
/// and, should the invariant ever break, degrades to the valid prefix instead of
/// panicking in the render path.
fn segment(bytes: &[u8]) -> &str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or(""),
    }
}

/// What a double click groups: a run of one class of characters is one unit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Whitespace,
    Punctuation,
}

fn char_class(character: char) -> CharClass {
    if character.is_alphanumeric() || character == '_' {
        CharClass::Word
    } else if character.is_whitespace() {
        CharClass::Whitespace
    } else {
        CharClass::Punctuation
    }
}

/// The unit a double click at `offset` selects in `text`: the run of word characters
/// (letters, digits, `_`), whitespace or punctuation the character there belongs to —
/// the character after `offset`, or before it at the end of the text or a line.
pub fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let after = text[offset..].chars().next().filter(|next| *next != '\n');
    let (start, class) = match after {
        Some(character) => (offset, char_class(character)),
        None => match text[..offset].chars().next_back() {
            Some(character) if character != '\n' => {
                (offset - character.len_utf8(), char_class(character))
            }
            _ => return offset..offset,
        },
    };
    let same = |character: &char| *character != '\n' && char_class(*character) == class;
    let run_start = start
        - text[..start]
            .chars()
            .rev()
            .take_while(same)
            .map(char::len_utf8)
            .sum::<usize>();
    let run_end = start
        + text[start..]
            .chars()
            .take_while(same)
            .map(char::len_utf8)
            .sum::<usize>();
    run_start..run_end
}

/// The unit a triple click at `offset` selects in `text`: its line, with the line
/// break that ends it, so a drag over whole lines copies them as lines.
pub fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index + 1);
    start..end
}

#[cfg(test)]
#[path = "buffer_tests.rs"]
mod tests;
