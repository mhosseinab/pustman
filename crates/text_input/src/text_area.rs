//! The multi-line [`TextArea`]: the same [`EditState`] as [`crate::input::TextInput`] —
//! typing, selection, clipboard, undo and the anchored highlights are one
//! implementation — wrapped to the widget's width, with line motion and an optional
//! line-number gutter. Enter inserts a line break instead of submitting, and the
//! widget answers to its own [`TEXT_AREA_CONTEXT`] key context (`crate::init`).
//!
//! `Decision:` the widget fills the box its owner gives it and does not scroll yet: a
//! body editor sizes it, and a caret outside the visible rows is simply clipped. A
//! scrolling viewport is a later step and owns keeping the caret in view.
//!
//! `Decision:` up and down move by *logical* lines (see [`EditState::line_up`]), not by
//! wrapped rows: the goal column is then plain buffer arithmetic that never depends on
//! a shape that may predate the last keystroke, and the tests are exact. Visual-row
//! motion can come with the scrolling viewport, when the wrapped layout is a first
//! class part of the geometry anyway.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    Bounds, CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Role, ShapedLine,
    Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, fill, point,
    px, relative, size,
};
use ui::prelude::*;

use crate::editing::Record;
use crate::highlights::Highlight;
use crate::input::{
    Backspace, Copy, Cut, Delete, Down, EditState, End, Enter, Home, Left, Paste, Redo, Right,
    SelectAll, SelectLeft, SelectRight, SelectToEnd, SelectToHome, SelectWordLeft, SelectWordRight,
    TextInputEvent, Undo, Up, WordLeft, WordRight, offset_for_hit_outside_text,
};

/// The key context the widget's editing keys bind to (`crate::init`).
pub const TEXT_AREA_CONTEXT: &str = "TextArea";

/// A multi-line text area.
pub struct TextArea {
    state: EditState,
    line_numbers: bool,
    // The wrapped layout and the last painted bounds live in cells shared with the
    // element (`Rc`, not entity borrows): the element paints the layout while the
    // platform holds `&mut App`, and paint may not hold an entity borrow across that.
    // They are written while painting and never through an entity update.
    last_layout: Rc<RefCell<Option<AreaLayout>>>,
    last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    shaped_revision: Cell<u64>,
    shaped_width: Cell<Option<Pixels>>,
}

impl TextArea {
    /// A new, empty text area.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            state: EditState::new(cx, true),
            line_numbers: false,
            last_layout: Rc::new(RefCell::new(None)),
            last_bounds: Rc::new(Cell::new(None)),
            shaped_revision: Cell::new(0),
            shaped_width: Cell::new(None),
        }
    }

    /// Show a line-number gutter down the left edge.
    pub fn with_line_numbers(mut self, line_numbers: bool) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Turn editing on or off. Motion and selection keep working either way.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.state.set_read_only(read_only);
        cx.notify();
    }

    /// The text area's text.
    pub fn text(&self) -> String {
        self.state.text()
    }

    /// The bounds the element painted at last frame — what a layout test measures the
    /// widget's painted size through (`text_area_layout_tests`, S33n).
    pub fn painted_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds.get()
    }

    /// Replace the text area's text, put the cursor at the end and report the change.
    /// Line breaks are kept (CRLF folds to LF, see [`EditState::set_text`]).
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

    /// Re-shape the text when it or the wrap width changed. The layout the paint, the
    /// mouse and the IME read is rebuilt at most once per change, never per frame.
    fn refresh_layout(&self, width: Pixels, window: &Window, cx: &App) {
        let revision = self.state.revision();
        if self.shaped_revision.get() == revision && self.shaped_width.get() == Some(width) {
            return;
        }

        let text = self.state.text();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let gutter_gap = DynamicSpacing::Base04.px(cx);
        let (gutter_width, numbers_width, numbers) = if self.line_numbers {
            let line_count = self.state.line_count();
            let widest = SharedString::from(line_count.to_string());
            let number_run = TextRun {
                len: widest.len(),
                font: style.font(),
                color: Color::Muted.color(cx),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let widest_number = window.text_system().shape_line(
                widest,
                font_size,
                std::slice::from_ref(&number_run),
                None,
            );
            let number_width = widest_number.width();
            let mut numbers = Vec::with_capacity(line_count);
            for number in 1..=line_count {
                let digits = SharedString::from(number.to_string());
                let run = TextRun {
                    len: digits.len(),
                    ..number_run.clone()
                };
                numbers.push(
                    window
                        .text_system()
                        .shape_line(digits, font_size, &[run], None),
                );
            }
            (number_width + gutter_gap, number_width, numbers)
        } else {
            (px(0.), px(0.), Vec::new())
        };

        let base_run = TextRun {
            len: text.len(),
            font: style.font(),
            color: Color::Default.color(cx),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // The IME's composing text is underlined, the way every platform draws it.
        let marked = self.state.marked_range().map(|range| {
            (
                range,
                UnderlineStyle {
                    color: Some(base_run.color),
                    thickness: px_to_rems(1.).to_pixels(window.rem_size()),
                    wavy: false,
                },
            )
        });
        let runs = self.state.runs(&base_run, marked);
        // Soft wrap is GPUI's wrapping text layout, cut at the width the widget has.
        let lines = match window.text_system().shape_text(
            SharedString::from(text),
            font_size,
            &runs,
            Some((width - gutter_width).max(px(1.))),
            None,
        ) {
            Ok(lines) => lines.into_vec(),
            Err(error) => {
                log::error!("text_input: wrapping the text area's text failed: {error}");
                Vec::new()
            }
        };

        let mut line_ranges = Vec::with_capacity(lines.len());
        let mut rows = Vec::with_capacity(lines.len());
        let mut line_start = 0;
        for line in &lines {
            let line_end = line_start + line.len();
            line_ranges.push(line_start..line_end);
            rows.push(visual_rows(line));
            // The wrapping layout split on the line break and dropped it.
            line_start = line_end + 1;
        }

        *self.last_layout.borrow_mut() = Some(AreaLayout {
            gutter_width,
            numbers_width,
            numbers,
            lines,
            line_ranges,
            rows,
        });
        self.shaped_revision.set(revision);
        self.shaped_width.set(Some(width));
    }

    /// The offset under a mouse position, through the layout from the last paint.
    /// Before the first paint there is no geometry to hit, so the caret goes to the
    /// text start.
    fn index_for_mouse_position(&self, position: Point<Pixels>, line_height: Pixels) -> usize {
        let Some(bounds) = self.last_bounds.get() else {
            return 0;
        };
        let last_layout = self.last_layout.borrow();
        let Some(layout) = last_layout.as_ref() else {
            return 0;
        };
        layout.index_for_point(
            position.x - bounds.left() - layout.gutter_width,
            position.y - bounds.top(),
            line_height,
        )
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

    /// `Home` is the start of the caret's line here — the multi-line widget's line
    /// motion — while [`crate::input::TextInput`] takes it to the text start.
    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.state.move_to_line_start(window, cx);
    }

    /// `End` is the end of the caret's line, before its line break.
    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.state.move_to_line_end(window, cx);
    }

    fn select_to_home(&mut self, _: &SelectToHome, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_to_line_start(window, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.state.select_to_line_end(window, cx);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.state.line_up(window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.state.line_down(window, cx);
    }

    /// `Decision:` Enter inserts a line break — this is the multi-line widget;
    /// [`crate::input::TextInput`] submits instead. It records as its own undo step, so
    /// a paragraph break undoes on its own and not with the typing around it.
    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        self.state.insert_text("\n", Record::Atomic, window, cx);
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
        let offset = self.index_for_mouse_position(event.position, window.line_height());
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
        let offset = self.index_for_mouse_position(event.position, window.line_height());
        self.state.drag_select(offset, window, cx);
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.state.focus_handle().clone()
    }
}

impl EventEmitter<TextInputEvent> for TextArea {}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        Some(self.state.text_for_range_utf16(range_utf16, actual_range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(self.state.selection_utf16())
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.set_selection_utf16(range_utf16, window, cx);
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.state.text_length_utf16())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.state.marked_range_utf16()
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
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.shaped_revision.get() != self.state.revision() {
            return None;
        }
        let last_layout = self.last_layout.borrow();
        let layout = last_layout.as_ref()?;
        let range = self.state.range_from_utf16(&range_utf16);
        let line_height = window.line_height();
        let start = layout.point_for_index(range.start, line_height);
        let end = layout.point_for_index(range.end, line_height);
        let text_left = bounds.left() + layout.gutter_width;
        Some(Bounds::from_corners(
            point(text_left + start.x, bounds.top() + start.y),
            point(text_left + end.x, bounds.top() + end.y + line_height),
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
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.shaped_revision.get() != self.state.revision() {
            return None;
        }
        let bounds = self.last_bounds.get()?;
        let last_layout = self.last_layout.borrow();
        let layout = last_layout.as_ref()?;
        let offset = layout.index_for_point(
            position.x - bounds.left() - layout.gutter_width,
            position.y - bounds.top(),
            window.line_height(),
        );
        Some(self.state.offset_to_utf16(offset))
    }
}

/// The wrapped layout one refresh computed: one [`WrappedLine`] per logical line, the
/// byte range each line covers, and the visual rows each line breaks into. Paint, mouse
/// hit-testing and the IME all read it.
struct AreaLayout {
    gutter_width: Pixels,
    numbers_width: Pixels,
    numbers: Vec<ShapedLine>,
    lines: Vec<WrappedLine>,
    line_ranges: Vec<Range<usize>>,
    rows: Vec<Vec<Range<usize>>>,
}

impl AreaLayout {
    /// The y of a logical line's first row, counted from the area's top.
    fn line_top(&self, line_index: usize, line_height: Pixels) -> Pixels {
        let rows: usize = self
            .rows
            .iter()
            .take(line_index)
            .map(|rows| rows.len())
            .sum();
        line_height * rows
    }

    /// The line `offset` falls on — an offset on a line break belongs to the line it
    /// ends.
    fn line_index_for_offset(&self, offset: usize) -> usize {
        self.line_ranges
            .iter()
            .position(|range| offset <= range.end)
            .unwrap_or_else(|| self.line_ranges.len().saturating_sub(1))
    }

    /// Where `offset` sits: x past the gutter, y down from the area's top. An offset on
    /// a wrap boundary lands at the start of the row it wraps to — that is where the
    /// caret goes as text wraps.
    fn point_for_index(&self, offset: usize, line_height: Pixels) -> Point<Pixels> {
        let line_index = self.line_index_for_offset(offset);
        let (Some(line), Some(range), Some(rows)) = (
            self.lines.get(line_index),
            self.line_ranges.get(line_index),
            self.rows.get(line_index),
        ) else {
            return point(px(0.), px(0.));
        };
        let local = offset.clamp(range.start, range.end) - range.start;
        let row_index = rows
            .iter()
            .position(|row| local < row.end)
            .unwrap_or_else(|| rows.len().saturating_sub(1));
        let position = row_point(line, rows, row_index, local, line_height);
        point(
            position.x,
            self.line_top(line_index, line_height) + position.y,
        )
    }

    /// The offset under `(x, y)` — x past the gutter, y down from the area's top. A
    /// position outside the text clamps to the nearest end of it.
    fn index_for_point(&self, x: Pixels, y: Pixels, line_height: Pixels) -> usize {
        let text_height = line_height * self.rows.iter().map(|rows| rows.len()).sum::<usize>();
        if let Some(offset) = offset_for_hit_outside_text(y, text_height, self.text_len()) {
            return offset;
        }
        let mut line_top = px(0.);
        for (line_index, rows) in self.rows.iter().enumerate() {
            let height = line_height * rows.len();
            if y < line_top + height || line_index + 1 == self.rows.len() {
                let (Some(line), Some(range)) =
                    (self.lines.get(line_index), self.line_ranges.get(line_index))
                else {
                    break;
                };
                let local = line
                    .closest_index_for_position(point(x, y - line_top), line_height)
                    .unwrap_or_else(|index| index);
                return range.start + local.min(range.end - range.start);
            }
            line_top += height;
        }
        self.text_len()
    }

    /// The text's byte length — where a hit past the last row lands.
    fn text_len(&self) -> usize {
        self.line_ranges.last().map(|range| range.end).unwrap_or(0)
    }

    /// One quad per visual row the selection covers, laid out behind the text.
    fn selection_quads(
        &self,
        selection: &Range<usize>,
        origin: Point<Pixels>,
        line_height: Pixels,
        color: Hsla,
    ) -> Vec<PaintQuad> {
        let mut quads = Vec::new();
        for (line_index, range) in self.line_ranges.iter().enumerate() {
            let start = selection.start.clamp(range.start, range.end);
            let end = selection.end.clamp(range.start, range.end);
            if start >= end {
                continue;
            }
            let (Some(line), Some(rows)) = (self.lines.get(line_index), self.rows.get(line_index))
            else {
                continue;
            };
            let line_top = self.line_top(line_index, line_height);
            for (row_index, row) in rows.iter().enumerate() {
                let row_start = (start - range.start).max(row.start);
                let row_end = (end - range.start).min(row.end);
                if row_start >= row_end {
                    continue;
                }
                let from = row_point(line, rows, row_index, row_start, line_height);
                let to = row_point(line, rows, row_index, row_end, line_height);
                quads.push(fill(
                    Bounds::from_corners(
                        point(
                            origin.x + self.gutter_width + from.x,
                            origin.y + line_top + from.y,
                        ),
                        point(
                            origin.x + self.gutter_width + to.x,
                            origin.y + line_top + from.y + line_height,
                        ),
                    ),
                    color,
                ));
            }
        }
        quads
    }
}

/// Where `local` offset sits in `line`'s row `row_index`: x within the row and y within
/// the line. A wrapped row starts at x 0 even though the unwrapped geometry of that
/// offset belongs to the row before it.
fn row_point(
    line: &WrappedLine,
    rows: &[Range<usize>],
    row_index: usize,
    local: usize,
    line_height: Pixels,
) -> Point<Pixels> {
    let row_start = rows.get(row_index).map(|row| row.start).unwrap_or(0);
    if local == row_start && row_index > 0 {
        return point(px(0.), line_height * row_index);
    }
    line.position_for_index(local, line_height)
        .unwrap_or_else(|| point(px(0.), line_height * row_index))
}

/// The byte ranges of the visual rows a wrapped line breaks into — offsets relative to
/// the line's own text, cut where the text system wrapped it.
fn visual_rows(line: &WrappedLine) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0;
    for boundary in line.wrap_boundaries() {
        let end = line
            .runs()
            .get(boundary.run_ix)
            .and_then(|run| run.glyphs.get(boundary.glyph_ix))
            .map(|glyph| glyph.index)
            .unwrap_or_else(|| line.len());
        rows.push(start..end);
        start = end;
    }
    rows.push(start..line.len());
    rows
}

/// The text block itself: refreshes the wrapped layout, paints the gutter, the rows,
/// the selection and the caret, and hands the platform an [`ElementInputHandler`] in
/// `paint` so IME and pointer queries reach the entity with this element's bounds.
struct TextAreaElement {
    area: Entity<TextArea>,
    last_layout: Rc<RefCell<Option<AreaLayout>>>,
    last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

struct PrepaintState {
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    /// The text area's AccessKit role. A custom element declares its own semantics —
    /// the text value it exposes is the wrapped layout's (see
    /// `a11y_synthetic_children` for the run-level shape a later step can add).
    fn a11y_role(&self) -> Option<Role> {
        Some(Role::MultilineTextInput)
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // The text area fills the box its owner gives it; scrolling to follow the caret
        // comes with the scrolling viewport.
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
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
        let area = self.area.read(cx);
        area.refresh_layout(bounds.size.width, window, cx);
        let line_height = window.line_height();
        let last_layout = self.last_layout.borrow();
        let Some(layout) = last_layout.as_ref() else {
            return PrepaintState {
                cursor: None,
                selection: Vec::new(),
            };
        };

        let selected_range = area.state.selected_range();
        let selection = if selected_range.is_empty() {
            Vec::new()
        } else {
            layout.selection_quads(
                &selected_range,
                bounds.origin,
                line_height,
                cx.theme().colors().element_selection_background,
            )
        };
        let cursor = selected_range.is_empty().then(|| {
            let position = layout.point_for_index(area.state.cursor_offset(), line_height);
            let cursor_width = px_to_rems(2.).to_pixels(window.rem_size());
            fill(
                Bounds::new(
                    point(
                        bounds.left() + layout.gutter_width + position.x,
                        bounds.top() + position.y,
                    ),
                    size(cursor_width, line_height),
                ),
                Color::Accent.color(cx),
            )
        });

        PrepaintState { cursor, selection }
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
        let focus_handle = self.area.read(cx).state.focus_handle().clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let line_height = window.line_height();
        let last_layout = self.last_layout.borrow();
        let Some(layout) = last_layout.as_ref() else {
            return;
        };
        for (line_index, line) in layout.lines.iter().enumerate() {
            let origin = point(
                bounds.left() + layout.gutter_width,
                bounds.top() + layout.line_top(line_index, line_height),
            );
            if let Err(error) =
                line.paint_background(origin, line_height, TextAlign::Left, None, window, cx)
            {
                log::error!("text_input: painting a text area line's background failed: {error}");
            }
            if let Err(error) = line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
                log::error!("text_input: painting a text area line failed: {error}");
            }
        }
        for (line_index, number) in layout.numbers.iter().enumerate() {
            let origin = point(
                bounds.left(),
                bounds.top() + layout.line_top(line_index, line_height),
            );
            if let Err(error) = number.paint(
                origin,
                line_height,
                TextAlign::Right,
                Some(layout.numbers_width),
                window,
                cx,
            ) {
                log::error!("text_input: painting a text area line number failed: {error}");
            }
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        // Paint is the only place that knows the geometry the mouse and the IME ask
        // about later, so it records it here — through the shared cells, never through
        // an entity update.
        self.last_bounds.set(Some(bounds));
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.state.focus_handle().is_focused(window);
        div()
            .key_context(TEXT_AREA_CONTEXT)
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
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
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
            // The widget fills the box its owner gives it (`TextAreaElement` sizes
            // itself `relative(1.)` of this root), so the root takes the box's size:
            // 100 % of an unsized root is 0 px, and a 0 px editor cannot be typed into
            // or seen (S33n).
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .relative()
            .child(TextAreaElement {
                area: cx.entity(),
                last_layout: self.last_layout.clone(),
                last_bounds: self.last_bounds.clone(),
            })
            .when(focused, |this| this.child(crate::focus_ring(cx)))
    }
}

#[cfg(test)]
#[path = "text_area_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "text_area_font_size_tests.rs"]
mod font_size_tests;

#[cfg(test)]
#[path = "text_area_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "text_area_multi_click_tests.rs"]
mod multi_click_tests;
