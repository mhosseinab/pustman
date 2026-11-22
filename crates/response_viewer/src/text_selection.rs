//! Selecting text by hand the way an editor does, shared by the body's lines and the
//! Headers, Cookies and Timeline tables: a press places the start, a double click takes
//! a word, a triple click a line (a table row), more take everything, and a drag grows
//! the selection by the unit the press selected.
//!
//! The body selects over its lines. A table selects over its cells, flattened in reading
//! order into one list of "lines", each followed by the text the copy puts between it and
//! the next one (`": "` between a header's name and value, `"\t"` between cookie columns,
//! `"\n"` at a row's end).

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    ClipboardItem, KeyBinding, MouseButton, Pixels, Point, Stateful, StyledText, TextLayout,
    WeakEntity, point,
};
use text_input::buffer::word_range_at;
use text_input::input::{Copy, SelectAll};
use ui::prelude::*;

use crate::body_selection::{BodyPoint, BodySelection, clamp_column};

/// The key context of the Headers, Cookies and Timeline tabs: copy and select-all act on
/// the hand-made selection over their cells.
pub(crate) const TEXT_CONTEXT: &str = "ResponseText";

/// Copy and select-all over the tabs' cells, on each platform's usual keys.
pub(crate) fn init(cx: &mut App) {
    let (copy, select_all) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-a")
    } else {
        ("ctrl-c", "ctrl-a")
    };
    cx.bind_keys([
        KeyBinding::new(copy, Copy, Some(TEXT_CONTEXT)),
        KeyBinding::new(select_all, SelectAll, Some(TEXT_CONTEXT)),
    ]);
}

/// What a selecting press selects by: characters, or the word or line it landed on (as
/// the range a drag must keep), or everything.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SelectUnit {
    Character,
    Word((BodyPoint, BodyPoint)),
    Line((BodyPoint, BodyPoint)),
    All,
}

/// A selection made by hand over some lines, and the press that is making it.
///
/// No [`Debug`](std::fmt::Debug): the lines are response text — header values, cookies
/// (PRD §7).
#[derive(Clone)]
pub(crate) struct TextSelector {
    selection: Option<BodySelection>,
    /// What the press that started the selection selects by, so a drag grows by it.
    unit: SelectUnit,
    /// Whether a drag is extending the selection right now.
    selecting: bool,
}

impl Default for TextSelector {
    fn default() -> Self {
        Self {
            selection: None,
            unit: SelectUnit::Character,
            selecting: false,
        }
    }
}

impl TextSelector {
    /// The selection, whatever lines it was made on.
    pub fn selection(&self) -> Option<&BodySelection> {
        self.selection.as_ref()
    }

    /// Whether a drag is extending the selection right now.
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    /// A press at `at` on `lines`, as editors take one: a single click places the start,
    /// a double click selects the word, a triple click the line with its break, and more
    /// select everything. With `extend` (Shift) it moves the end of the current selection.
    /// `breaks` marks where lines are cells of one row (see the module docs); [`None`]
    /// makes every line its own row.
    pub fn begin(
        &mut self,
        lines: Arc<Vec<SharedString>>,
        breaks: Option<&[&'static str]>,
        at: BodyPoint,
        extend: bool,
        click_count: usize,
    ) {
        match self.selection.as_mut() {
            Some(selection) if extend && Arc::ptr_eq(&selection.lines, &lines) => {
                selection.head = at;
                self.unit = SelectUnit::Character;
            }
            _ => {
                self.unit = match click_count {
                    0 | 1 => SelectUnit::Character,
                    2 => SelectUnit::Word(word_at(&lines, at)),
                    3 => SelectUnit::Line(line_at(&lines, breaks, at)),
                    _ => SelectUnit::All,
                };
                self.selection = Some(match &self.unit {
                    SelectUnit::Character => BodySelection {
                        lines,
                        anchor: at,
                        head: at,
                    },
                    SelectUnit::Word((start, end)) | SelectUnit::Line((start, end)) => {
                        BodySelection {
                            lines,
                            anchor: *start,
                            head: *end,
                        }
                    }
                    SelectUnit::All => BodySelection::all(lines),
                });
            }
        }
        self.selecting = true;
    }

    /// A drag to `at`: move the selection's end there, or — after a double or triple
    /// click — select from the pressed word or line through the one under the pointer.
    /// True when the selection changed.
    pub fn extend(&mut self, breaks: Option<&[&'static str]>, at: BodyPoint) -> bool {
        if !self.selecting {
            return false;
        }
        let Some(selection) = self.selection.as_mut() else {
            return false;
        };
        let (anchor, head) = match &self.unit {
            SelectUnit::Character => (selection.anchor, at),
            SelectUnit::All => return false,
            SelectUnit::Word(pressed) | SelectUnit::Line(pressed) => {
                let (start, end) = *pressed;
                let (under_start, under_end) = match self.unit {
                    SelectUnit::Word(_) => word_at(&selection.lines, at),
                    _ => line_at(&selection.lines, breaks, at),
                };
                if under_start < start {
                    (end, under_start)
                } else {
                    (start, end.max(under_end))
                }
            }
        };
        if selection.anchor == anchor && selection.head == head {
            return false;
        }
        selection.anchor = anchor;
        selection.head = head;
        true
    }

    /// The press that was selecting is released.
    pub fn end(&mut self) {
        self.selecting = false;
    }

    /// Everything in `lines`.
    pub fn select_all(&mut self, lines: Arc<Vec<SharedString>>) {
        self.selection = Some(BodySelection::all(lines));
    }
}

/// The selectable cells of one table, flattened in reading order, and the selection over
/// them. The cells are rebuilt when the table's data changes, which leaves an older
/// selection behind.
///
/// No [`Debug`](std::fmt::Debug), like [`TextSelector`]: the cells hold header and cookie
/// values (PRD §7).
#[derive(Clone)]
pub(crate) struct CellSelector {
    cells: Arc<Vec<SharedString>>,
    /// What the copy puts after each cell (the module docs).
    breaks: Arc<Vec<&'static str>>,
    selector: TextSelector,
}

impl Default for CellSelector {
    fn default() -> Self {
        Self {
            cells: Arc::new(Vec::new()),
            breaks: Arc::new(Vec::new()),
            selector: TextSelector::default(),
        }
    }
}

impl CellSelector {
    /// Replace the cells: each one with the text that follows it in a copy.
    pub fn set(&mut self, cells: Vec<(SharedString, &'static str)>) {
        let (cells, breaks) = cells.into_iter().unzip();
        self.cells = Arc::new(cells);
        self.breaks = Arc::new(breaks);
        self.selector = TextSelector::default();
    }

    pub fn begin(&mut self, at: BodyPoint, extend: bool, click_count: usize) {
        self.selector.begin(
            Arc::clone(&self.cells),
            Some(&self.breaks),
            at,
            extend,
            click_count,
        );
    }

    /// True when the selection changed.
    pub fn extend(&mut self, at: BodyPoint) -> bool {
        self.selector.extend(Some(&self.breaks), at)
    }

    pub fn end(&mut self) {
        self.selector.end();
    }

    pub fn select_all(&mut self) {
        self.selector.select_all(Arc::clone(&self.cells));
    }

    fn active(&self) -> Option<&BodySelection> {
        self.selector
            .selection()
            .filter(|selection| !selection.is_empty() && Arc::ptr_eq(&selection.lines, &self.cells))
    }

    /// The selected text, cells joined by their breaks.
    pub fn selected_text(&self) -> Option<String> {
        self.active()
            .map(|selection| joined_text(selection, &self.breaks))
    }

    /// The selected bytes of cell `index`, if any of it is selected.
    pub fn range_on(&self, index: usize) -> Option<Range<usize>> {
        self.active()?.range_on(index)
    }

    /// Copy the selection, if there is one.
    pub fn copy(&self, cx: &mut App) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
}

/// A view whose cells are selectable: the table views hand the shared cell renderer
/// their [`CellSelector`] through this.
pub(crate) trait SelectableCells: 'static {
    fn cell_selector(&mut self) -> &mut CellSelector;
}

/// The tab root's half of cell selection: copy and select-all act on the selection, and a
/// release anywhere — over the tab or outside it — ends the press that was selecting.
/// The caller adds the key context ([`TEXT_CONTEXT`]) and a focus handle.
pub(crate) fn selection_handlers<V: SelectableCells, E: InteractiveElement>(
    root: E,
    cx: &mut Context<V>,
) -> E {
    root.on_action(cx.listener(|view, _: &Copy, _window, cx| {
        view.cell_selector().copy(cx);
    }))
    .on_action(cx.listener(|view, _: &SelectAll, _window, cx| {
        view.cell_selector().select_all();
        cx.notify();
    }))
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(|view, _event, _window, _cx| view.cell_selector().end()),
    )
    .on_mouse_up_out(
        MouseButton::Left,
        cx.listener(|view, _event, _window, _cx| view.cell_selector().end()),
    )
}

/// One selectable table cell: its text with the selection painted behind it, truncated
/// with the full value in a tooltip. A press selects from it and a drag over it extends
/// the selection. Callers add the font, width and colour.
pub(crate) fn selectable_cell<V: SelectableCells>(
    id: ElementId,
    index: usize,
    text: SharedString,
    selected: Option<Range<usize>>,
    view: WeakEntity<V>,
    cx: &App,
) -> Stateful<Div> {
    let highlights = selected
        .map(|range| {
            let style = gpui::HighlightStyle {
                background_color: Some(cx.theme().colors().element_selection_background),
                ..gpui::HighlightStyle::default()
            };
            vec![(range, style)]
        })
        .unwrap_or_default();
    let styled = StyledText::new(text.clone()).with_highlights(highlights);
    let press_layout = styled.layout().clone();
    let drag_layout = press_layout.clone();
    let press_text = text.clone();
    let drag_text = text.clone();
    let drag_view = view.clone();
    div()
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .min_w_0()
        .tooltip(Tooltip::text(text).builder())
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            let at = BodyPoint {
                line: index,
                column: column_at(&press_layout, &press_text, event.position),
            };
            let extend = event.modifiers.shift;
            let click_count = event.click_count;
            if view
                .update(cx, |view, cx| {
                    view.cell_selector().begin(at, extend, click_count);
                    cx.notify();
                })
                .is_err()
            {
                // The tab closed under the pointer; there is nothing left to select.
            }
        })
        .on_mouse_move(move |event, _window, cx| {
            if event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let at = BodyPoint {
                line: index,
                column: column_at(&drag_layout, &drag_text, event.position),
            };
            if drag_view
                .update(cx, |view, cx| {
                    if view.cell_selector().extend(at) {
                        cx.notify();
                    }
                })
                .is_err()
            {
                // The tab closed under the pointer; there is nothing left to select.
            }
        })
        .child(div().w_full().min_w_0().truncate().child(styled))
}

/// The selection's text, each line followed by its break up to the last selected one;
/// with no breaks, lines join by `\n`.
fn joined_text(selection: &BodySelection, breaks: &[&'static str]) -> String {
    let (start, end) = if selection.anchor <= selection.head {
        (selection.anchor, selection.head)
    } else {
        (selection.head, selection.anchor)
    };
    let mut text = String::new();
    for index in start.line..=end.line {
        let Some(line) = selection.lines.get(index) else {
            break;
        };
        let from = if index == start.line { start.column } else { 0 };
        let to = if index == end.line {
            end.column
        } else {
            line.len()
        };
        if let Some(part) = line.get(from..to) {
            text.push_str(part);
        }
        if index < end.line {
            text.push_str(breaks.get(index).copied().unwrap_or("\n"));
        }
    }
    text
}

/// The byte column of `line` under the pointer's x. The text is one visual line, so the
/// pointer's y is pinned to it: a drag that wanders above or below the text still selects
/// by x.
pub(crate) fn column_at(layout: &TextLayout, line: &str, position: Point<Pixels>) -> usize {
    let at = point(position.x, layout.bounds().center().y);
    let index = match layout.index_for_position(at) {
        Ok(index) | Err(index) => index,
    };
    clamp_column(line, index)
}

/// The word, run of punctuation or gap under `at`, as the text inputs see one.
fn word_at(lines: &[SharedString], at: BodyPoint) -> (BodyPoint, BodyPoint) {
    let line = lines.get(at.line).map_or("", |line| line.as_ref());
    let range = word_range_at(line, at.column);
    (
        BodyPoint {
            line: at.line,
            column: range.start,
        },
        BodyPoint {
            line: at.line,
            column: range.end,
        },
    )
}

/// The row under `at` with its line break: from its first line through the start of the
/// next row, or to the end of the last one. Without `breaks` every line is a row; with
/// them a row runs to the line followed by `"\n"`.
fn line_at(
    lines: &[SharedString],
    breaks: Option<&[&'static str]>,
    at: BodyPoint,
) -> (BodyPoint, BodyPoint) {
    let ends_row = |index: usize| breaks.is_none_or(|breaks| breaks.get(index) == Some(&"\n"));
    let first = (0..=at.line)
        .rev()
        .find(|&index| index == 0 || ends_row(index - 1))
        .unwrap_or(0);
    let last = (at.line..lines.len())
        .find(|&index| ends_row(index))
        .unwrap_or_else(|| lines.len().saturating_sub(1));
    let start = BodyPoint {
        line: first,
        column: 0,
    };
    if last + 1 < lines.len() {
        (
            start,
            BodyPoint {
                line: last + 1,
                column: 0,
            },
        )
    } else {
        (start, end_of_line(lines, last))
    }
}

pub(crate) fn end_of_line(lines: &[SharedString], line: usize) -> BodyPoint {
    BodyPoint {
        line,
        column: lines.get(line).map_or(0, |text| text.len()),
    }
}
