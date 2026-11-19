//! The single-line [`TextInput`]: a [`crate::buffer::Buffer`] under a GPUI entity that
//! speaks IME through [`EntityInputHandler`], moves the cursor by grapheme and word,
//! selects with shift, shows a placeholder when empty, can be read-only, and reports its
//! life through [`TextInputEvent`]. The shape is GPUI's Apache-licensed
//! `examples/input.rs` — including its UTF-16 ⇄ UTF-8 offset conversions, because that
//! is the encoding the platform's text input speaks — and the code is ours.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::ops::Range;

use gpui::{
    Bounds, ClipboardItem, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, Focusable, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Point, Role, ShapedLine, Style,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, actions, fill, point, relative, size,
};
use ui::prelude::*;

use crate::buffer::Buffer;
use crate::editing::{Record, Selection, Transaction, UndoStack};

/// The key context the widget's editing keys bind to (`crate::init`). This is the
/// widget's own context — like Zed's `Editor` — not a PRD action context.
pub const TEXT_INPUT_CONTEXT: &str = "TextInput";

/// The debug selector the placeholder carries, so a test can see it come and go.
pub const PLACEHOLDER_SELECTOR: &str = "text-input-placeholder";

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
    /// The user pressed Enter in the field.
    Submitted,
}

impl EventEmitter<TextInputEvent> for TextInput {}

/// A single-line text field.
pub struct TextInput {
    buffer: Buffer,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    undo_stack: UndoStack,
    placeholder: SharedString,
    read_only: bool,
    focus_handle: FocusHandle,
    is_selecting: bool,
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
            buffer: Buffer::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            undo_stack: UndoStack::new(),
            placeholder: SharedString::default(),
            read_only: false,
            focus_handle: cx.focus_handle(),
            is_selecting: false,
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

    /// Turn editing on or off. Motion and selection keep working either way.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        cx.notify();
    }

    /// The field's text.
    pub fn text(&self) -> String {
        self.buffer.text().into_owned()
    }

    /// Replace the field's text, put the cursor at the end and report the change.
    ///
    /// `Decision:` this emits [`TextInputEvent::Changed`] like any other edit: a
    /// subscriber that only cares about user input (validation, dirty state) should not
    /// have to know which path changed the text, and a subscriber that set the text
    /// itself can ignore its own event.
    ///
    /// `Decision:` it also clears the undo history — a programmatic replacement is not
    /// a user edit, so there is nothing to undo into.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = single_line(text);
        self.buffer.replace(0..self.buffer.len(), &text);
        let end = self.buffer.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.undo_stack.clear();
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    /// The selected byte range; a caret is an empty range at its offset.
    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
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

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        let offset = offset.min(self.buffer.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
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
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    /// The caret offset under a mouse position, through the shaped line from the last
    /// paint. Before the first paint there is no geometry to hit, so the caret goes to
    /// the text start.
    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.buffer.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.get() else {
            return 0;
        };
        let last_layout = self.last_layout.borrow();
        let Some(line) = last_layout.as_ref() else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.buffer.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    /// A UTF-16 offset from the platform, as the buffer's UTF-8 byte offset.
    fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        utf8_offset_for_utf16(self.buffer.chars(), utf16_offset)
    }

    /// A buffer byte offset as the UTF-16 offset the platform speaks.
    fn offset_to_utf16(&self, utf8_offset: usize) -> usize {
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

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous = self.buffer.previous_boundary(self.cursor_offset());
            self.move_to(previous, window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.buffer.next_boundary(self.cursor_offset());
            self.move_to(next, window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.buffer.previous_boundary(self.cursor_offset());
        self.select_to(previous, window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.buffer.next_boundary(self.cursor_offset());
        self.select_to(next, window, cx);
    }

    fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.buffer.previous_word_start(self.cursor_offset());
        self.move_to(previous, window, cx);
    }

    fn word_right(&mut self, _: &WordRight, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.buffer.next_word_end(self.cursor_offset());
        self.move_to(next, window, cx);
    }

    fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = self.buffer.previous_word_start(self.cursor_offset());
        self.select_to(previous, window, cx);
    }

    fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.buffer.next_word_end(self.cursor_offset());
        self.select_to(next, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.buffer.len(), window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.buffer.len(), window, cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, window, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.buffer.len(), window, cx);
    }

    /// The one path every text mutation takes: replace `range` with `text`, put the
    /// selection where `selection_after` says — a caret right after the inserted text
    /// when it is `None` — and record the edit in the [`UndoStack`] the way `record`
    /// says. Typing coalesces, paste and cut stand alone, an IME's composition becomes
    /// one transaction, and undo/redo move transactions instead of recording.
    fn edit(
        &mut self,
        range: Range<usize>,
        text: &str,
        selection_after: Option<Selection>,
        record: Record,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection_before = self.selection();
        let old_text = self.buffer.slice(range.clone());
        let text = single_line(text);
        self.buffer.replace(range.clone(), &text);
        self.marked_range = None;
        let selection_after =
            selection_after.unwrap_or_else(|| Selection::caret(range.start + text.len()));
        self.set_selection(selection_after);
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

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
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

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
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
    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let text = self.buffer.slice(self.selected_range.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// `Decision:` cut with no selection is a no-op too, like copy; cut and paste are
    /// no-ops in read-only mode.
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.selected_range.is_empty() {
            return;
        }
        let text = self.buffer.slice(self.selected_range.clone());
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let range = self.selected_range.clone();
        self.edit(range, "", None, Record::Atomic, window, cx);
    }

    /// `Decision:` paste replaces the selection and leaves the caret after the pasted
    /// text; `single_line` turns pasted line breaks into spaces.
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
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
    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
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

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
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

    /// `Decision:` Enter submits and never inserts a newline — this is the single-line
    /// field; a multi-line `TextArea` is S20c. It emits [`TextInputEvent::Submitted`]
    /// even in read-only mode: reading a value does not stop the user from confirming it.
    fn enter(&mut self, _: &Enter, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submitted);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.request_virtual_keyboard();
        window.focus(&self.focus_handle, cx);
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
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
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.buffer.slice(range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_selection(Selection {
            range: self.range_from_utf16(&range_utf16),
            reversed: false,
        });
        self.undo_stack.break_coalescing();
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(
            self.buffer
                .chars()
                .map(|character| character.len_utf16())
                .sum(),
        )
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        // The composition ended without a commit: its updates still deserve one undo
        // step.
        self.undo_stack.finish_composition();
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
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

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let new_text = single_line(new_text);
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
        // commits (see `replace_text_in_range`).
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

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.shaped_revision.get() != self.buffer.revision() {
            return None;
        }
        let last_layout = self.last_layout.borrow();
        let line = last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
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
        if self.shaped_revision.get() != self.buffer.revision() {
            return None;
        }
        let bounds = self.last_bounds.get()?;
        let line_point = bounds.localize(&position)?;
        let last_layout = self.last_layout.borrow();
        let line = last_layout.as_ref()?;
        Some(self.offset_to_utf16(line.closest_index_for_x(line_point.x)))
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
        let content = input.buffer.text().into_owned();
        let style = window.text_style();
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), Color::Placeholder.color(cx))
        } else {
            (SharedString::from(content), Color::Default.color(cx))
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // The IME's composing text is underlined, the way every platform draws it.
        let runs = if let Some(marked_range) = input.marked_range.clone() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px_to_rems(1.).to_pixels(window.rem_size()),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_width = px_to_rems(2.).to_pixels(window.rem_size());
        let (selection, cursor) = if input.selected_range.is_empty() {
            let cursor_x = line.x_for_index(input.cursor_offset());
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
                            bounds.left() + line.x_for_index(input.selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(input.selected_range.end),
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
        let focus_handle = self.input.read(cx).focus_handle.clone();
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
        input.shaped_revision.set(input.buffer.revision());
        input.last_bounds.set(Some(bounds));
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(TEXT_INPUT_CONTEXT)
            .track_focus(&self.focus_handle)
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
            .text_ui_size(TextSize::Ui, cx)
            .text_color(Color::Default.color(cx))
            .px(DynamicSpacing::Base04.px(cx))
            .min_w_0()
            .overflow_hidden()
            .relative()
            // The placeholder is a label behind the text line, not part of the shaped
            // line: the caret still shows in the empty field, and a test can watch the
            // placeholder come and go.
            .children(self.buffer.is_empty().then(|| {
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .debug_selector(|| PLACEHOLDER_SELECTOR.to_string())
                    .text_color(Color::Placeholder.color(cx))
                    .child(self.placeholder.clone())
            }))
            .child(TextElement { input: cx.entity() })
    }
}

/// A single-line input never stores line breaks: CRLF, CR and LF in pasted or IME text
/// become one space each (Enter submits; a multi-line field is S20c's `TextArea`).
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

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
