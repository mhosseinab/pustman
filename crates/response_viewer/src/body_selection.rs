//! A hand-made selection over the body's lines: where a drag started and where it is
//! now, both as `(line, byte column)` points into the lines it was made on.

use std::ops::Range;
use std::sync::Arc;

use gpui::SharedString;

/// A caret position in the drawn lines: a line index and a byte column on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BodyPoint {
    pub line: usize,
    pub column: usize,
}

/// The selection, tied to the lines it was made on: Pretty and Raw draw different lines,
/// so a selection only applies while those exact lines are on screen.
#[derive(Clone, Debug)]
pub(crate) struct BodySelection {
    pub lines: Arc<Vec<SharedString>>,
    pub anchor: BodyPoint,
    pub head: BodyPoint,
}

impl BodySelection {
    /// Everything in `lines`, first byte to last.
    pub fn all(lines: Arc<Vec<SharedString>>) -> Self {
        let head = BodyPoint {
            line: lines.len().saturating_sub(1),
            column: lines.last().map_or(0, |line| line.len()),
        };
        Self {
            lines,
            anchor: BodyPoint { line: 0, column: 0 },
            head,
        }
    }

    fn ordered(&self) -> (BodyPoint, BodyPoint) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selected bytes of line `index`, if any of it is selected.
    pub fn range_on(&self, index: usize) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if self.is_empty() || index < start.line || index > end.line {
            return None;
        }
        let length = self.lines.get(index).map_or(0, |line| line.len());
        let from = if index == start.line { start.column } else { 0 };
        let to = if index == end.line {
            end.column
        } else {
            length
        };
        (from < to).then_some(from..to)
    }

    /// The selected text, lines joined by `\n`.
    pub fn text(&self) -> String {
        let (start, end) = self.ordered();
        let mut text = String::new();
        for index in start.line..=end.line {
            let Some(line) = self.lines.get(index) else {
                break;
            };
            if index > start.line {
                text.push('\n');
            }
            let from = if index == start.line { start.column } else { 0 };
            let to = if index == end.line {
                end.column
            } else {
                line.len()
            };
            if let Some(part) = line.get(from..to) {
                text.push_str(part);
            }
        }
        text
    }
}

/// `column` clamped onto `line`: never past its end, never inside a character.
pub(crate) fn clamp_column(line: &str, column: usize) -> usize {
    line.floor_char_boundary(column.min(line.len()))
}
