//! The editing substrate of [`TextInput`] and [`crate::text_area::TextArea`], and the
//! single-line [`TextInput`] built on it. [`EditState`] holds a [`crate::buffer::Buffer`]
//! behind the small surface a text field needs — the selection, the IME's marked range,
//! the [`UndoStack`], the anchored [`crate::highlights::HighlightSet`] — and the one
//! `edit` path every mutation takes. Both widgets are thin shells over it: their own
//! rendering, their own key context and their own Enter key. The shape is GPUI's
//! Apache-licensed `examples/input.rs` — including its UTF-16 ⇄ UTF-8 offset
//! conversions, because that is the encoding the platform's text input speaks — and the
//! code is ours.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Point, Role, ShapedLine,
    Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, actions, fill, point, relative,
    size,
};
use ui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::{Buffer, line_range_at, word_range_at};
use crate::editing::{Record, Selection, Transaction, UndoStack};
use crate::highlights::{Highlight, HighlightSet};

/// The key context the single-line widget's editing keys bind to (`crate::init`). This
/// is the widget's own context — like Zed's `Editor` — not a PRD action context.
pub const TEXT_INPUT_CONTEXT: &str = "TextInput";

/// The debug selector the placeholder carries, so a test can see it come and go.
pub const PLACEHOLDER_SELECTOR: &str = "text-input-placeholder";

/// Masked mode's stand-in character ([`TextInput::with_masked`]): one stands per
/// character of a masked field's text. It is one UTF-16 unit in the offsets the
/// platform speaks and uniformly three UTF-8 bytes in the shaped line, so every
/// display offset is a whole number of masks.
const MASK_CHARACTER: char = '\u{2022}';

/// Test hook: the single-line field's own box, under `VisualTestContext::debug_bounds`.
pub const TEXT_INPUT_SELECTOR: &str = "text-input";

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectToHome,
        SelectToEnd,
        /// Moves the caret one line up.
        Up,
        /// Moves the caret one line down.
        Down,
        Enter,
        /// Copies the selection to the clipboard.
        Copy,
        /// Cuts the selection to the clipboard.
        Cut,
        /// Pastes the clipboard over the selection.
        Paste,
        /// Undoes the last edit.
        Undo,
        /// Redoes the last undone edit.
        Redo,
    ]
);

/// What the field tells its owner about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The text changed through typing, an edit, `set_text` or the IME.
    Changed,
    /// The user pressed Enter in the single-line field.
    Submitted,
}

impl EventEmitter<TextInputEvent> for TextInput {}

/// The editing state behind [`TextInput`] and [`crate::text_area::TextArea`]: the
/// buffer, the selection, the IME's marked range, the undo stack, the highlight anchors
/// and the one mutation path every change to the text goes through. Widgets delegate
/// their actions here, so typing, selection, clipboard and undo behave the same in both
/// and there is exactly one place the text can change.
///
/// `Decision:` the state owns the [`FocusHandle`] and reaches its widget through the
/// [`EventEmitter`] bound — it emits [`TextInputEvent::Changed`] and notifies at the
/// moment of the mutation — instead of taking callbacks. A widget then cannot forget to
/// tell its owner that the text changed, and both widgets report the same event.
pub struct EditState {
    buffer: Buffer,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    undo_stack: UndoStack,
    highlights: HighlightSet,
    read_only: bool,
    multiline: bool,
    goal_column: Option<usize>,
    is_selecting: bool,
    select_unit: SelectUnit,
    focus_handle: FocusHandle,
}

/// What a mouse selection grows by, from the click count that started it — Zed's
/// editor shape: one click places the caret, two take a word, three a line, four all.
/// A word or line unit keeps the range the click took, so a drag selects the union of
/// it and the unit under the pointer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum SelectUnit {
    #[default]
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
    All,
}

impl EditState {
    /// Editing state for a new widget. `multiline` is the widget's line mode: it decides
    /// what happens to line breaks in text on its way in (see [`Self::normalize`]).
    pub fn new(cx: &App, multiline: bool) -> Self {
        Self {
            buffer: Buffer::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            undo_stack: UndoStack::new(),
            highlights: HighlightSet::new(),
            read_only: false,
            multiline,
            goal_column: None,
            is_selecting: false,
            select_unit: SelectUnit::Character,
            focus_handle: cx.focus_handle(),
        }
    }

    /// The widget's focus handle.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// The text.
    pub fn text(&self) -> String {
        self.buffer.text().into_owned()
    }

    /// Whether there is no text at all.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// The text's length in bytes.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// A counter bumped by every edit. A paint cache records it to know whether the
    /// layout it shaped is still the current text.
    pub fn revision(&self) -> u64 {
        self.buffer.revision()
    }

    /// The text in `range`.
    pub fn slice(&self, range: Range<usize>) -> String {
        self.buffer.slice(range)
    }

    /// How many logical lines the text has — a trailing line break starts one more.
    pub fn line_count(&self) -> usize {
        1 + self
            .buffer
            .chars()
            .filter(|character| *character == '\n')
            .count()
    }

    /// The selected byte range; a caret is an empty range at its offset.
    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    /// Where the caret is: the selection's start when it was made backwards, its end
    /// otherwise.
    pub fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// The IME's composing span, in UTF-8 byte offsets.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    /// The anchored highlight spans.
    pub fn highlights(&self) -> &[Highlight] {
        self.highlights.spans()
    }

    /// Replace the highlight spans. They are clamped to the text; see
    /// [`crate::highlights`] for how they move with later edits.
    pub fn set_highlights(&mut self, highlights: Vec<Highlight>) {
        self.highlights.set(highlights, self.buffer.text().as_ref());
    }

    /// The [`TextRun`]s that paint `base`'s text under the highlight spans and the IME
    /// composition's `marked` underline.
    pub fn runs(
        &self,
        base: &TextRun,
        marked: Option<(Range<usize>, UnderlineStyle)>,
    ) -> Vec<TextRun> {
        self.highlights.runs(base, marked)
    }

    /// Turn editing on or off. Motion and selection keep working either way.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Replace the whole text, put the caret at the end, and clear the undo history and
    /// the highlight spans.
    ///
    /// `Decision:` a programmatic replacement is not a user edit, so there is nothing to
    /// undo into — and the spans described the old text, so the caller re-highlights
    /// what matters. The widget emits [`TextInputEvent::Changed`] for the change (see
    /// [`TextInput::set_text`]).
    pub fn set_text(&mut self, text: &str) {
        let text = self.normalize(text);
        self.buffer.replace(0..self.buffer.len(), &text);
        let end = self.buffer.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.undo_stack.clear();
        self.highlights.clear();
        self.goal_column = None;
    }

    /// The selection as an undo-model snapshot.
    fn selection(&self) -> Selection {
        Selection {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
        }
    }

    /// Put the selection at `selection`, clamped to the text so a stale offset from an
    /// undo transaction can never point past the buffer.
    fn set_selection(&mut self, selection: Selection) {
        let end = self.buffer.len();
        let start = selection.range.start.min(end);
        self.selected_range = start..selection.range.end.clamp(start, end);
        self.selection_reversed = selection.reversed;
    }

    /// Put the caret at `offset` — any motion but the line motions clears the goal
    /// column they aim for.
    pub fn move_to<E: EventEmitter<TextInputEvent>>(
        &mut self,
        offset: usize,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let offset = offset.min(self.buffer.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.goal_column = None;
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// Grow the selection to `offset`, from the end the caret is at.
    pub fn select_to<E: EventEmitter<TextInputEvent>>(
        &mut self,
        offset: usize,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let offset = offset.min(self.buffer.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.goal_column = None;
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// Move the caret one grapheme left, or collapse the selection to its start.
    pub fn left<E: EventEmitter<TextInputEvent>>(&mut self, window: &Window, cx: &mut Context<E>) {
        if self.selected_range.is_empty() {
            let previous = self.buffer.previous_boundary(self.cursor_offset());
            self.move_to(previous, window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    /// Move the caret one grapheme right, or collapse the selection to its end.
    pub fn right<E: EventEmitter<TextInputEvent>>(&mut self, window: &Window, cx: &mut Context<E>) {
        if self.selected_range.is_empty() {
            let next = self.buffer.next_boundary(self.cursor_offset());
            self.move_to(next, window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    /// Grow the selection one grapheme left.
    pub fn select_left<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let previous = self.buffer.previous_boundary(self.cursor_offset());
        self.select_to(previous, window, cx);
    }

    /// Grow the selection one grapheme right.
    pub fn select_right<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let next = self.buffer.next_boundary(self.cursor_offset());
        self.select_to(next, window, cx);
    }

    /// Move the caret one word left.
    pub fn word_left<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let previous = self.buffer.previous_word_start(self.cursor_offset());
        self.move_to(previous, window, cx);
    }

    /// Move the caret one word right.
    pub fn word_right<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let next = self.buffer.next_word_end(self.cursor_offset());
        self.move_to(next, window, cx);
    }

    /// Grow the selection one word left.
    pub fn select_word_left<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let previous = self.buffer.previous_word_start(self.cursor_offset());
        self.select_to(previous, window, cx);
    }

    /// Grow the selection one word right.
    pub fn select_word_right<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let next = self.buffer.next_word_end(self.cursor_offset());
        self.select_to(next, window, cx);
    }

    /// Select the whole text.
    pub fn select_all<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        self.move_to(0, window, cx);
        self.select_to(self.buffer.len(), window, cx);
    }

    /// Move the caret to the text start.
    pub fn home<E: EventEmitter<TextInputEvent>>(&mut self, window: &Window, cx: &mut Context<E>) {
        self.move_to(0, window, cx);
    }

    /// Move the caret to the text end.
    pub fn end<E: EventEmitter<TextInputEvent>>(&mut self, window: &Window, cx: &mut Context<E>) {
        self.move_to(self.buffer.len(), window, cx);
    }

    /// Grow the selection to the text start.
    pub fn select_to_home<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        self.select_to(0, window, cx);
    }

    /// Grow the selection to the text end.
    pub fn select_to_end<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        self.select_to(self.buffer.len(), window, cx);
    }

    /// Move the caret to the start of the line it is on.
    pub fn move_to_line_start<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let start = self.line_start(self.cursor_offset());
        self.move_to(start, window, cx);
    }

    /// Move the caret to the end of the line it is on (before the line break).
    pub fn move_to_line_end<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let end = self.line_end(self.cursor_offset());
        self.move_to(end, window, cx);
    }

    /// Grow the selection to the start of the caret's line.
    pub fn select_to_line_start<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let start = self.line_start(self.cursor_offset());
        self.select_to(start, window, cx);
    }

    /// Grow the selection to the end of the caret's line.
    pub fn select_to_line_end<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let end = self.line_end(self.cursor_offset());
        self.select_to(end, window, cx);
    }

    /// Move the caret one line up.
    ///
    /// `Decision:` the caret keeps a goal column across the lines it crosses — a short
    /// line clamps the caret to its end for the moment, and the next line restores the
    /// column — and on the first line Up lands at the text start, the way every editor
    /// does it.
    pub fn line_up<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let cursor = self.cursor_offset();
        let start = self.line_start(cursor);
        if start == 0 {
            self.move_to(0, window, cx);
            return;
        }
        let column = self
            .goal_column
            .unwrap_or_else(|| self.column_at(start, cursor));
        let target_line = self.line_start(start - 1);
        let target = self.offset_at_column(target_line, column);
        self.move_to(target, window, cx);
        self.goal_column = Some(column);
    }

    /// Move the caret one line down, keeping the same goal column as [`Self::line_up`].
    /// On the last line Down lands at the text end.
    pub fn line_down<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        let cursor = self.cursor_offset();
        let end = self.line_end(cursor);
        if end >= self.buffer.len() {
            self.move_to(self.buffer.len(), window, cx);
            return;
        }
        let column = self
            .goal_column
            .unwrap_or_else(|| self.column_at(self.line_start(cursor), cursor));
        let target = self.offset_at_column(end + 1, column);
        self.move_to(target, window, cx);
        self.goal_column = Some(column);
    }

    /// The start of the line `offset` is on.
    pub fn line_start(&self, offset: usize) -> usize {
        let offset = offset.min(self.buffer.len());
        match self.buffer.slice(0..offset).rfind('\n') {
            Some(index) => index + 1,
            None => 0,
        }
    }

    /// The end of the line `offset` is on — the line break's offset, or the text end.
    pub fn line_end(&self, offset: usize) -> usize {
        let offset = offset.min(self.buffer.len());
        match self.buffer.slice(offset..self.buffer.len()).find('\n') {
            Some(index) => offset + index,
            None => self.buffer.len(),
        }
    }

    /// How many graphemes into its line the caret at `offset` is.
    fn column_at(&self, line_start: usize, offset: usize) -> usize {
        self.buffer
            .slice(line_start..offset.max(line_start))
            .graphemes(true)
            .count()
    }

    /// The offset `column` graphemes into the line starting at `line_start`, clamped to
    /// that line's end.
    fn offset_at_column(&self, line_start: usize, column: usize) -> usize {
        let end = self.line_end(line_start);
        self.buffer
            .slice(line_start..end)
            .grapheme_indices(true)
            .nth(column)
            .map(|(index, _)| line_start + index)
            .unwrap_or(end)
    }

    /// The one path every text mutation takes: replace `range` with `text`, put the
    /// selection where `selection_after` says — a caret right after the inserted text
    /// when it is `None` — and record the edit in the [`UndoStack`] the way `record`
    /// says. Typing coalesces, paste and cut stand alone, an IME's composition becomes
    /// one transaction, and undo/redo move transactions instead of recording. The
    /// highlight anchors move through the same edit, so they track the text.
    pub fn edit<E: EventEmitter<TextInputEvent>>(
        &mut self,
        range: Range<usize>,
        text: &str,
        selection_after: Option<Selection>,
        record: Record,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        let selection_before = self.selection();
        let old_text = self.buffer.slice(range.clone());
        let text = self.normalize(text);
        self.buffer.replace(range.clone(), &text);
        self.marked_range = None;
        self.highlights
            .shift_for_edit(&range, text.len(), self.buffer.text().as_ref());
        let selection_after =
            selection_after.unwrap_or_else(|| Selection::caret(range.start + text.len()));
        self.set_selection(selection_after);
        self.goal_column = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextInputEvent::Changed);
        cx.notify();
        self.undo_stack.record(
            Transaction {
                replaced_range: range,
                old_text,
                new_text: text.into_owned(),
                selection_before,
                selection_after: self.selection(),
            },
            record,
        );
    }

    /// Replace the selection with `text` — what Enter and paste-type insertions need.
    pub fn insert_text<E: EventEmitter<TextInputEvent>>(
        &mut self,
        text: &str,
        record: Record,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let range = self.selected_range.clone();
        self.edit(range, text, None, record, window, cx);
    }

    /// Delete the grapheme before the caret, or the whole selection.
    pub fn backspace<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let range = if self.selected_range.is_empty() {
            let previous = self.buffer.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == previous {
                window.play_system_bell();
                return;
            }
            previous..self.cursor_offset()
        } else {
            self.selected_range.clone()
        };
        self.edit(range, "", None, Record::Atomic, window, cx);
    }

    /// Delete the grapheme after the caret, or the whole selection.
    pub fn delete<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let range = if self.selected_range.is_empty() {
            let next = self.buffer.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                window.play_system_bell();
                return;
            }
            self.cursor_offset()..next
        } else {
            self.selected_range.clone()
        };
        self.edit(range, "", None, Record::Atomic, window, cx);
    }

    /// `Decision:` copy with no selection is a no-op — it must not clobber a clipboard
    /// holding something else — and copy works in read-only mode.
    pub fn copy<E: EventEmitter<TextInputEvent>>(&mut self, cx: &mut Context<E>) {
        if self.selected_range.is_empty() {
            return;
        }
        let text = self.buffer.slice(self.selected_range.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// `Decision:` cut with no selection is a no-op too, like copy; cut and paste are
    /// no-ops in read-only mode.
    pub fn cut<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only || self.selected_range.is_empty() {
            return;
        }
        let text = self.buffer.slice(self.selected_range.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let range = self.selected_range.clone();
        self.edit(range, "", None, Record::Atomic, window, cx);
    }

    /// `Decision:` paste replaces the selection and leaves the caret after the pasted
    /// text. What happens to line breaks in the pasted text is the widget's line mode
    /// (see [`Self::normalize`]).
    pub fn paste<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let range = self.selected_range.clone();
        self.edit(range, &text, None, Record::Atomic, window, cx);
    }

    /// `Decision:` undo and redo are no-ops in read-only mode — the text must never
    /// change there — and undoing into an empty stack rings the bell instead.
    pub fn undo<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let Some(transaction) = self.undo_stack.undo() else {
            window.play_system_bell();
            return;
        };
        self.edit(
            transaction.new_text_range(),
            &transaction.old_text,
            Some(transaction.selection_before.clone()),
            Record::None,
            window,
            cx,
        );
    }

    /// Redo the last undone edit.
    pub fn redo<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let Some(transaction) = self.undo_stack.redo() else {
            window.play_system_bell();
            return;
        };
        self.edit(
            transaction.replaced_range.clone(),
            &transaction.new_text,
            Some(transaction.selection_after.clone()),
            Record::None,
            window,
            cx,
        );
    }

    /// Start a mouse selection at `offset` — `extend` grows the selection the way Shift
    /// does instead of moving the caret; `click_count` picks the unit (see
    /// [`SelectUnit`]).
    pub fn begin_selecting<E: EventEmitter<TextInputEvent>>(
        &mut self,
        offset: usize,
        extend: bool,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        self.is_selecting = true;
        window.request_virtual_keyboard();
        window.focus(&self.focus_handle, cx);
        if extend {
            self.select_unit = SelectUnit::Character;
            self.select_to(offset, window, cx);
            return;
        }
        self.select_unit = match click_count {
            0 | 1 => SelectUnit::Character,
            2 => SelectUnit::Word(self.word_range_at(offset)),
            3 => SelectUnit::Line(self.line_range_at(offset)),
            _ => SelectUnit::All,
        };
        match self.select_unit.clone() {
            SelectUnit::Character => self.move_to(offset, window, cx),
            SelectUnit::Word(range) | SelectUnit::Line(range) => {
                self.select_range(range, false, window, cx);
            }
            SelectUnit::All => self.select_range(0..self.buffer.len(), false, window, cx),
        }
    }

    /// Grow the mouse selection to `offset`, if a mouse selection is under way: by
    /// characters, or by the word or line unit a multi-click started it with.
    pub fn drag_select<E: EventEmitter<TextInputEvent>>(
        &mut self,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if !self.is_selecting {
            return;
        }
        let (original, under) = match self.select_unit.clone() {
            SelectUnit::Character => {
                self.select_to(offset, window, cx);
                return;
            }
            SelectUnit::All => return,
            SelectUnit::Word(original) => (original, self.word_range_at(offset)),
            SelectUnit::Line(original) => (original, self.line_range_at(offset)),
        };
        let range = original.start.min(under.start)..original.end.max(under.end);
        self.select_range(range, under.start < original.start, window, cx);
    }

    /// End the mouse selection.
    pub fn stop_selecting(&mut self) {
        self.is_selecting = false;
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        word_range_at(&self.buffer.text(), offset)
    }

    /// A single-line field is one line, so a triple click takes all of it.
    fn line_range_at(&self, offset: usize) -> Range<usize> {
        if self.multiline {
            line_range_at(&self.buffer.text(), offset)
        } else {
            0..self.buffer.len()
        }
    }

    fn select_range<E: EventEmitter<TextInputEvent>>(
        &mut self,
        range: Range<usize>,
        reversed: bool,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        self.set_selection(Selection { range, reversed });
        self.goal_column = None;
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// A UTF-16 offset from the platform, as the buffer's UTF-8 byte offset.
    pub fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        utf8_offset_for_utf16(self.buffer.chars(), utf16_offset)
    }

    /// A buffer byte offset as the UTF-16 offset the platform speaks.
    pub fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.buffer.chars() {
            if utf8_count >= utf8_offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    pub fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    /// The number of characters before `offset` — masked mode's display metric: one
    /// [`MASK_CHARACTER`] stands per character of the text (see
    /// [`TextInput::with_masked`]).
    pub fn char_count_before(&self, offset: usize) -> usize {
        let mut count = 0;
        let mut byte_offset = 0;
        for character in self.buffer.chars() {
            if byte_offset >= offset {
                break;
            }
            byte_offset += character.len_utf8();
            count += 1;
        }
        count
    }

    /// [`Self::char_count_before`]'s inverse: the byte offset of the `index`th
    /// character, clamped to the text.
    pub fn offset_of_char(&self, index: usize) -> usize {
        self.buffer.chars().take(index).map(char::len_utf8).sum()
    }

    /// The text in a UTF-16 range, with the platform's view of the range that was read.
    pub fn text_for_range_utf16(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
    ) -> String {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        self.buffer.slice(range)
    }

    /// The selection, in the offsets the platform speaks.
    pub fn selection_utf16(&self) -> UTF16Selection {
        UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        }
    }

    /// Put the selection where the platform's input method asks for it.
    pub fn set_selection_utf16<E: EventEmitter<TextInputEvent>>(
        &mut self,
        range_utf16: Range<usize>,
        window: &Window,
        cx: &mut Context<E>,
    ) {
        self.set_selection(Selection {
            range: self.range_from_utf16(&range_utf16),
            reversed: false,
        });
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// The text's length in the offsets the platform speaks.
    pub fn text_length_utf16(&self) -> usize {
        self.buffer
            .chars()
            .map(|character| character.len_utf16())
            .sum()
    }

    /// The IME's composing span, in the offsets the platform speaks.
    pub fn marked_range_utf16(&self) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    /// End the IME composition without a commit.
    pub fn unmark_text<E: EventEmitter<TextInputEvent>>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        self.marked_range = None;
        // The composition ended without a commit: its updates still deserve one undo
        // step.
        self.undo_stack.finish_composition();
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    /// The IME's committed text, replacing a UTF-16 range.
    pub fn replace_text_in_range_utf16<E: EventEmitter<TextInputEvent>>(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        // An IME's commit is the last update of its composition: it merges into the
        // composition's transaction and closes it, so a composition undoes as one
        // edit. Plain input is typing.
        let record = if self.undo_stack.has_composition() {
            Record::Composition
        } else {
            Record::Typing
        };
        self.edit(range, new_text, None, record, window, cx);
        self.undo_stack.finish_composition();
    }

    /// The IME's composing text update: replaces a UTF-16 range and marks what it put
    /// there as the composition in progress.
    pub fn replace_and_mark_text_in_range_utf16<E: EventEmitter<TextInputEvent>>(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<E>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let new_text = self.normalize(new_text);
        // The platform's selection is relative to the text it just inserted, so it maps
        // through that text — not through the buffer it is about to land in.
        let selection_after = new_selected_range_utf16.as_ref().map(|selected| {
            let inserted_length_utf16 = new_text.encode_utf16().count();
            let start =
                utf8_offset_for_utf16(new_text.chars(), selected.start.min(inserted_length_utf16));
            let end =
                utf8_offset_for_utf16(new_text.chars(), selected.end.min(inserted_length_utf16));
            Selection {
                range: range.start + start..range.start + end,
                reversed: false,
            }
        });
        // A composition update is not an edit of its own: it merges into the
        // composition's transaction, which is recorded once when the composition
        // commits (see `replace_text_in_range_utf16`).
        self.edit(
            range.clone(),
            &new_text,
            selection_after,
            Record::Composition,
            window,
            cx,
        );
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
    }

    /// Text on its way in, in the widget's line mode: a single-line widget turns line
    /// breaks into spaces (it never stores a line break), a multi-line one keeps them
    /// and folds CRLF and lone CR into one LF.
    ///
    /// `Decision:` folding to LF — not stripping CR — keeps every stored offset a UTF-8
    /// byte offset on one kind of line break, so line motion and the highlight anchors
    /// have exactly one separator to reason about.
    fn normalize<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.multiline {
            normalize_newlines(text)
        } else {
            single_line(text)
        }
    }
}

/// A single-line input never stores line breaks: CRLF, CR and LF in pasted or IME text
/// become one space each (Enter submits; [`crate::text_area::TextArea`] is the
/// multi-line widget).
fn single_line(text: &str) -> Cow<'_, str> {
    if text.contains(['\r', '\n']) {
        let mut single = String::with_capacity(text.len());
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '\r' => {
                    if characters.peek() == Some(&'\n') {
                        characters.next();
                    }
                    single.push(' ');
                }
                '\n' => single.push(' '),
                _ => single.push(character),
            }
        }
        Cow::Owned(single)
    } else {
        Cow::Borrowed(text)
    }
}

/// Multi-line text stores one kind of line break: CRLF and lone CR fold into LF.
fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        let mut normalized = String::with_capacity(text.len());
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            if character == '\r' {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                normalized.push('\n');
            } else {
                normalized.push(character);
            }
        }
        Cow::Owned(normalized)
    } else {
        Cow::Borrowed(text)
    }
}

/// A UTF-16 offset from the platform, as a UTF-8 byte offset into `characters`' text.
/// The walk stops at the text end, so an out-of-range offset clamps instead of
/// panicking.
fn utf8_offset_for_utf16(characters: impl Iterator<Item = char>, utf16_offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in characters {
        if utf16_count >= utf16_offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

/// Where a pointer hit outside the text's rows lands — the vertical clamp
/// [`TextInput`] and [`crate::text_area::TextArea`] share: a hit above the text is its
/// start, one at or past its bottom is its end. `None` means the hit is on the text
/// and the caller hit-tests inside it.
pub(crate) fn offset_for_hit_outside_text(
    y: Pixels,
    text_height: Pixels,
    text_len: usize,
) -> Option<usize> {
    if y < px(0.) {
        Some(0)
    } else if y >= text_height {
        Some(text_len)
    } else {
        None
    }
}

/// A single-line text field.
pub struct TextInput {
    state: EditState,
    placeholder: SharedString,
    /// Masked mode: the shaped line and the platform read [`MASK_CHARACTER`]s, never
    /// the value ([`Self::with_masked`]).
    masked: bool,
    // The layout cache is plain interior mutability on purpose: the element learns the
    // text's geometry while painting and must record it without updating the entity
    // (never `Entity::update` from `render`/`paint`).
    last_layout: RefCell<Option<ShapedLine>>,
    last_bounds: Cell<Option<Bounds<Pixels>>>,
    shaped_revision: Cell<u64>,
}

impl TextInput {
    /// A new, empty field.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: EditState::new(cx, false),
            placeholder: SharedString::default(),
            masked: false,
            last_layout: RefCell::new(None),
            last_bounds: Cell::new(None),
            shaped_revision: Cell::new(0),
        }
    }

    /// Show `placeholder` while the field is empty.
    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Mask the field: the painted line and the platform read one [`MASK_CHARACTER`]
    /// per character of the text, never the value itself. Editing, selection and
    /// copying keep working on the real text — copying a password copies the password.
    ///
    /// `Decision:` masking is display-deep and lives in exactly the two doors that
    /// *show* text: the line the field shapes and the text it hands the platform
    /// (S06: identify auth "without exposing credentials"). One mask per character —
    /// not per byte — keeps the masks countable against what was typed. A masked field
    /// paints one uniform run: highlight colours and the IME's composing underline
    /// over masks would leak the shape of the value (S03 keeps secret reveal a
    /// separate concern, and it is not this mode).
    pub fn with_masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// Whether the field masks its value.
    pub fn is_masked(&self) -> bool {
        self.masked
    }

    /// The text the field shows: the value, or one [`MASK_CHARACTER`] per character of
    /// it in masked mode — exactly what the painted line shapes.
    pub fn display_text(&self) -> String {
        let text = self.state.text();
        if self.masked {
            text.chars().map(|_| MASK_CHARACTER).collect()
        } else {
            text
        }
    }

    /// Turn editing on or off. Motion and selection keep working either way.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.state.set_read_only(read_only);
        cx.notify();
    }

    /// The field's text.
    pub fn text(&self) -> String {
        self.state.text()
    }

    /// Replace the field's text, put the cursor at the end and report the change.
    ///
    /// `Decision:` this emits [`TextInputEvent::Changed`] like any other edit: a
    /// subscriber that only cares about user input (validation, dirty state) should not
    /// have to know which path changed the text, and a subscriber that set the text
    /// itself can ignore its own event.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state.set_text(text);
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// The selected byte range; a caret is an empty range at its offset.
    pub fn selected_range(&self) -> Range<usize> {
        self.state.selected_range()
    }

    /// The anchored highlight spans.
    pub fn highlights(&self) -> &[Highlight] {
        self.state.highlights()
    }

    /// Replace the highlight spans; see [`crate::highlights`] for how they move with
    /// later edits.
    pub fn set_highlights(&mut self, highlights: Vec<Highlight>, cx: &mut Context<Self>) {
        self.state.set_highlights(highlights);
        cx.notify();
    }

    /// The completion trigger at the caret ([`crate::completion`]) — what a field's
    /// autocomplete popover shows. The one read-only door onto that hook: the field
    /// side never sees the editing state itself.
    pub fn completion(&self) -> Option<crate::completion::Completion> {
        crate::completion::completion_at(&self.state)
    }

    /// Replace a completion's range with the chosen `name`, through the one edit path
    /// ([`crate::completion`]): one [`TextInputEvent::Changed`], undo-able like any edit.
    pub fn insert_completion(
        &mut self,
        completion: &crate::completion::Completion,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::completion::insert(&mut self.state, completion, name, window, cx);
    }

    /// A buffer byte offset as an offset into [`Self::display_text`]'s bytes — the
    /// indices [`ShapedLine`] speaks. Unmasked, the two are the same text.
    fn display_offset(&self, buffer_offset: usize) -> usize {
        if self.masked {
            self.state.char_count_before(buffer_offset) * MASK_CHARACTER.len_utf8()
        } else {
            buffer_offset
        }
    }

    /// [`Self::display_offset`]'s inverse: a display byte offset as the buffer byte
    /// offset it stands over, on the character boundary that owns it.
    fn buffer_offset(&self, display_offset: usize) -> usize {
        if self.masked {
            self.state
                .offset_of_char(display_offset / MASK_CHARACTER.len_utf8())
        } else {
            display_offset
        }
    }

    /// A buffer byte offset in the offsets the platform speaks: UTF-16 of the display
    /// text, which masked mode keeps the value out of.
    fn platform_offset(&self, buffer_offset: usize) -> usize {
        if self.masked {
            self.state.char_count_before(buffer_offset) * MASK_CHARACTER.len_utf16()
        } else {
            self.state.offset_to_utf16(buffer_offset)
        }
    }

    /// [`Self::platform_offset`]'s inverse for a whole range: what the platform's
    /// UTF-16 range means as buffer byte offsets.
    fn buffer_range_for_platform(&self, range_utf16: Range<usize>) -> Range<usize> {
        if self.masked {
            self.state
                .offset_of_char(range_utf16.start / MASK_CHARACTER.len_utf16())
                ..self
                    .state
                    .offset_of_char(range_utf16.end / MASK_CHARACTER.len_utf16())
        } else {
            self.state.range_from_utf16(&range_utf16)
        }
    }

    /// The caret offset under a mouse position, through the shaped line from the last
    /// paint. Before the first paint there is no geometry to hit, so the caret goes to
    /// the text start.
    ///
    /// Public as a read-only accessor for callers that hit-test the field's text from
    /// outside the field (the request editor's variable hover uses it to learn which
    /// placeholder the pointer is over); it changes nothing.
    pub fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.state.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.get() else {
            return 0;
        };
        let last_layout = self.last_layout.borrow();
        let Some(line) = last_layout.as_ref() else {
            return 0;
        };
        let y = position.y - bounds.top();
        if let Some(offset) = offset_for_hit_outside_text(y, bounds.size.height, self.state.len()) {
            return offset;
        }
        self.buffer_offset(line.closest_index_for_x(position.x - bounds.left()))
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.state.backspace(window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.state.delete(window, cx);
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        self.state.left(window, cx);
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        self.state.right(window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_left(window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_right(window, cx);
    }

    fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.state.word_left(window, cx);
    }

    fn word_right(&mut self, _: &WordRight, window: &mut Window, cx: &mut Context<Self>) {
        self.state.word_right(window, cx);
    }

    fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.select_word_left(window, cx);
    }

    fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.select_word_right(window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_all(window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.state.home(window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.state.end(window, cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_to_home(window, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_to_end(window, cx);
    }

    /// `Decision:` Enter submits and never inserts a newline — this is the single-line
    /// field. It emits [`TextInputEvent::Submitted`] even in read-only mode: reading a
    /// value does not stop the user from confirming it.
    fn enter(&mut self, _: &Enter, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submitted);
    }

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        self.state.copy(cx);
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.state.cut(window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        self.state.paste(window, cx);
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.state.undo(window, cx);
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        self.state.redo(window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.index_for_mouse_position(event.position);
        self.state
            .begin_selecting(offset, event.modifiers.shift, event.click_count, window, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.state.stop_selecting();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.index_for_mouse_position(event.position);
        self.state.drag_select(offset, window, cx);
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.masked {
            // The platform reads masks, never the value (`with_masked`).
            let range = self.buffer_range_for_platform(range_utf16);
            actual_range
                .replace(self.platform_offset(range.start)..self.platform_offset(range.end));
            let masks =
                self.state.char_count_before(range.end) - self.state.char_count_before(range.start);
            Some(MASK_CHARACTER.to_string().repeat(masks))
        } else {
            Some(self.state.text_for_range_utf16(range_utf16, actual_range))
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.state.selection_utf16();
        let range = self.state.selected_range();
        Some(UTF16Selection {
            range: self.platform_offset(range.start)..self.platform_offset(range.end),
            reversed: selection.reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.buffer_range_for_platform(range_utf16);
        self.state
            .set_selection_utf16(self.state.range_to_utf16(&range), window, cx);
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.platform_offset(self.state.len()))
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let range = self.state.marked_range()?;
        Some(self.platform_offset(range.start)..self.platform_offset(range.end))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.unmark_text(window, cx);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range_utf16 = range_utf16.map(|range_utf16| {
            let range = self.buffer_range_for_platform(range_utf16);
            self.state.range_to_utf16(&range)
        });
        self.state
            .replace_text_in_range_utf16(range_utf16, new_text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range_utf16 = range_utf16.map(|range_utf16| {
            let range = self.buffer_range_for_platform(range_utf16);
            self.state.range_to_utf16(&range)
        });
        // `new_selected_range_utf16` is relative to `new_text` — real text however the
        // field displays itself — so it maps through `new_text` unchanged (that mapping
        // is `EditState`'s).
        self.state.replace_and_mark_text_in_range_utf16(
            range_utf16,
            new_text,
            new_selected_range_utf16,
            window,
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.shaped_revision.get() != self.state.revision() {
            return None;
        }
        let last_layout = self.last_layout.borrow();
        let line = last_layout.as_ref()?;
        let range = self.buffer_range_for_platform(range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + line.x_for_index(self.display_offset(range.start)),
                bounds.top(),
            ),
            point(
                bounds.left() + line.x_for_index(self.display_offset(range.end)),
                bounds.bottom(),
            ),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.shaped_revision.get() != self.state.revision() {
            return None;
        }
        let bounds = self.last_bounds.get()?;
        let line_point = bounds.localize(&position)?;
        let last_layout = self.last_layout.borrow();
        let line = last_layout.as_ref()?;
        Some(self.platform_offset(self.buffer_offset(line.closest_index_for_x(line_point.x))))
    }
}

/// The text line itself: shapes the text, paints the selection and the caret, and hands
/// the platform an [`ElementInputHandler`] in `paint` so IME and pointer queries reach
/// the entity with this element's bounds.
struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    /// The field's AccessKit role. A custom element declares its own semantics — the
    /// text value it exposes is the shaped line's (see `a11y_synthetic_children` for
    /// the run-level shape a later step can add).
    fn a11y_role(&self) -> Option<Role> {
        Some(Role::TextInput)
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let state = &input.state;
        let style = window.text_style();
        // The placeholder is the label `TextInput::render` draws; shaping it here too
        // painted it twice.
        let display_text = SharedString::from(input.display_text());
        let text_color = Color::Default.color(cx);

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // Masked mode paints one uniform run: highlight colours and the composing
        // underline over masks would leak the shape of the value (`with_masked`).
        let runs = if input.masked {
            vec![run]
        } else {
            // The IME's composing text is underlined, the way every platform draws it.
            let marked = state.marked_range().map(|range| {
                (
                    range,
                    UnderlineStyle {
                        color: Some(run.color),
                        thickness: px_to_rems(1.).to_pixels(window.rem_size()),
                        wavy: false,
                    },
                )
            });
            state.runs(&run, marked)
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_width = px_to_rems(2.).to_pixels(window.rem_size());
        let selected_range = state.selected_range();
        let (selection, cursor) = if selected_range.is_empty() {
            let cursor_x = line.x_for_index(input.display_offset(state.cursor_offset()));
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        size(cursor_width, bounds.size.height),
                    ),
                    Color::Accent.color(cx),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left()
                                + line.x_for_index(input.display_offset(selected_range.start)),
                            bounds.top(),
                        ),
                        point(
                            bounds.left()
                                + line.x_for_index(input.display_offset(selected_range.end)),
                            bounds.bottom(),
                        ),
                    ),
                    cx.theme().colors().element_selection_background,
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).state.focus_handle().clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        if let Err(error) = line.paint(
            bounds.origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            cx,
        ) {
            log::error!("text_input: painting the field's text failed: {error}");
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        // Paint is the only place that knows the geometry the IME asks about later, so
        // it records that geometry here — through the entity's interior-mutable cache,
        // never through an entity update.
        let input = self.input.read(cx);
        *input.last_layout.borrow_mut() = Some(line);
        input.shaped_revision.set(input.state.revision());
        input.last_bounds.set(Some(bounds));
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.state.focus_handle().is_focused(window);
        div()
            .key_context(TEXT_INPUT_CONTEXT)
            .debug_selector(|| TEXT_INPUT_SELECTOR.to_string())
            .track_focus(self.state.focus_handle())
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_to_home))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .font_ui(cx)
            .text_ui_size(TextSize::Default, cx)
            .text_color(Color::Default.color(cx))
            .px(DynamicSpacing::Base04.px(cx))
            .min_w_0()
            .overflow_hidden()
            .relative()
            // The placeholder is a label behind the text line, not part of the shaped
            // line: the caret still shows in the empty field, and a test can watch the
            // placeholder come and go.
            .children(self.state.is_empty().then(|| {
                div()
                    .absolute()
                    // Absolute children ignore the parent's padding; match the text
                    // line's origin so the placeholder sits where typing starts.
                    .left(DynamicSpacing::Base04.px(cx))
                    .top_0()
                    .debug_selector(|| PLACEHOLDER_SELECTOR.to_string())
                    .text_color(Color::Placeholder.color(cx))
                    .child(self.placeholder.clone())
            }))
            .child(TextElement { input: cx.entity() })
            .when(focused, |this| this.child(crate::focus_ring(cx)))
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "input_placeholder_tests.rs"]
mod placeholder_tests;

#[cfg(test)]
#[path = "input_font_size_tests.rs"]
mod font_size_tests;

#[cfg(test)]
#[path = "input_multi_click_tests.rs"]
mod multi_click_tests;

#[cfg(test)]
#[path = "input_masked_tests.rs"]
mod masked_tests;
