//! The reusable key/value table behind the Params tab (REQ-HTTP-02): rows of
//! [`KeyValue`] with one stored [`TextInput`] per editable cell, an enabled box and
//! add/remove rows. The table owns the rows and reports every change as one
//! [`KeyValueTableEvent`]; the owner (the [`crate::params::Params`]) decides what a row
//! change means for the URL and the request draft.
//!
//! `Decision:` rows carry a stable per-row id that never changes while the row lives, and
//! every element id derives from it — editing or reordering must not move focus (trap 5).
//! The table writes no URL and knows no URL; `extra` on a row is preserved verbatim on
//! every rewrite (REQ-MODEL-02).

use api_model::KeyValue;
use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Subscription, Window,
};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;

/// The width of the checkbox and remove columns in pixels at the default scale: a fixed
/// column never squeezes the text columns next to it.
const NARROW_COLUMN_WIDTH_PX: f32 = 48.;

/// Which cell of a row is being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowField {
    Key,
    Value,
    Description,
}

/// How a table's rows are shaped: rows the user owns, or rows owned by something else (a
/// path-variable row's key comes from the URL).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyValueTableShape {
    /// Full rows: editable keys, an enabled box, add/remove rows — the params shape.
    Editable,
    /// Derived rows: keys are read-only and the rows are added and removed by the owner.
    Derived,
}

/// What a [`KeyValueTable`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyValueTableEvent {
    /// A row's text or enabled flag changed; [`KeyValueTable::rows`] is the new truth.
    RowsChanged,
}

impl EventEmitter<KeyValueTableEvent> for KeyValueTable {}

/// One row: the model value plus the cells that edit it. The cells are stored entities
/// (created once per row) so editing one keeps its focus and caret.
struct Row {
    /// The stable id every element id in this row derives from. Ids are never reused, so
    /// an event for a removed row can never land on a different one.
    id: u64,
    row: KeyValue,
    key_input: Entity<TextInput>,
    value_input: Entity<TextInput>,
    description_input: Entity<TextInput>,
    /// Dropped with the row: its cells' events stop being delivered with it.
    _subscriptions: Vec<Subscription>,
}

impl Row {
    /// The text the cell for `field` shows.
    fn cell_text(&self, field: RowField) -> String {
        match field {
            RowField::Key => self.row.key.clone(),
            RowField::Value => self.row.value.clone().unwrap_or_default(),
            RowField::Description => self.row.description.clone().unwrap_or_default(),
        }
    }

    /// Apply edited cell text. An empty value or description is `None` — a key-only row
    /// is what `?a` parses to and what it round-trips back to.
    fn apply_text(&mut self, field: RowField, text: String) {
        match field {
            RowField::Key => self.row.key = text,
            RowField::Value => self.row.value = (!text.is_empty()).then_some(text),
            RowField::Description => self.row.description = (!text.is_empty()).then_some(text),
        }
    }
}

/// A table of [`KeyValue`] rows: key, value, description, an enabled box, add/remove.
pub struct KeyValueTable {
    shape: KeyValueTableShape,
    rows: Vec<Row>,
    next_id: u64,
    /// The table's element-id root: two tables in one window never collide.
    focus_handle: FocusHandle,
}

impl KeyValueTable {
    /// An empty table. [`KeyValueTableShape`] decides which columns it draws.
    pub fn new(shape: KeyValueTableShape, cx: &mut Context<Self>) -> Self {
        Self {
            shape,
            rows: Vec::new(),
            next_id: 0,
            focus_handle: cx.focus_handle(),
        }
    }

    /// The rows as the request model stores them.
    pub fn rows(&self) -> Vec<KeyValue> {
        self.rows.iter().map(|row| row.row.clone()).collect()
    }

    /// Every row's cells with the row field each edits, in row order — the read-only
    /// reach the variable highlighter needs into this table's inputs
    /// (`crate::variable_highlight`, REQ-VAR-03).
    pub fn cell_inputs(&self) -> Vec<(RowField, Entity<TextInput>)> {
        self.rows
            .iter()
            .flat_map(|row| {
                [
                    (RowField::Key, row.key_input.clone()),
                    (RowField::Value, row.value_input.clone()),
                    (RowField::Description, row.description_input.clone()),
                ]
            })
            .collect()
    }

    /// The stable ids of the rows, in order — what a caller compares to prove that a
    /// round trip through the URL did not rebuild the table under the caret.
    pub fn row_ids(&self) -> Vec<u64> {
        self.rows.iter().map(|row| row.id).collect()
    }

    /// Replace the rows wholesale — the URL became the source of truth (its query
    /// changed). The old rows go away with their cells' subscriptions. `Decision:` this
    /// is the owner's own programmatic write, so it emits nothing — the owner announces
    /// it (or not) with its own event; only user edits announce themselves here.
    pub fn set_rows(&mut self, rows: Vec<KeyValue>, window: &mut Window, cx: &mut Context<Self>) {
        self.rows = Vec::with_capacity(rows.len());
        for row in rows {
            self.push_row(row, window, cx);
        }
    }

    /// Reconcile the rows to exactly `keys`, keeping each surviving row's id and values
    /// (a row that stays keeps what the user typed) and dropping rows whose key is gone.
    /// For rows whose keys come from elsewhere (`:name` path segments). Returns whether
    /// the row set changed — same programmatic-write rule as [`Self::set_rows`].
    pub fn sync_keys(
        &mut self,
        keys: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut previous: Vec<Option<Row>> = self.rows.drain(..).map(Some).collect();
        let mut merged = Vec::with_capacity(keys.len());
        let mut changed = false;
        for key in keys {
            let kept = previous
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|row| row.row.key == *key))
                .and_then(Option::take);
            match kept {
                Some(row) => merged.push(row),
                None => {
                    changed = true;
                    merged.push(self.make_row(
                        KeyValue {
                            key: key.clone(),
                            value: None,
                            description: None,
                            enabled: true,
                            extra: Default::default(),
                        },
                        window,
                        cx,
                    ));
                }
            }
        }
        if previous.iter().any(Option::is_some) {
            changed = true;
        }
        self.rows = merged;
        changed
    }

    /// Append an empty row and put the caret in its key cell. `Decision:` the empty row
    /// does not emit [`KeyValueTableEvent::RowsChanged`] yet — it says nothing about the
    /// request (no key, no value) and syncing it would only decorate the URL; the first
    /// edit to it syncs like any other row.
    pub fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.make_row(
            KeyValue {
                key: String::new(),
                value: None,
                description: None,
                enabled: true,
                extra: Default::default(),
            },
            window,
            cx,
        );
        let id = row.id;
        self.rows.push(row);
        self.focus_cell(id, RowField::Key, window, cx);
    }

    /// Remove the row with `id`.
    pub fn remove_row(&mut self, id: u64, cx: &mut Context<Self>) {
        self.rows.retain(|row| row.id != id);
        cx.emit(KeyValueTableEvent::RowsChanged);
    }

    /// Flip a row's enabled flag — the checkbox in its first cell.
    pub fn set_enabled(&mut self, id: u64, enabled: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        if row.row.enabled == enabled {
            return;
        }
        row.row.enabled = enabled;
        cx.emit(KeyValueTableEvent::RowsChanged);
    }

    /// Move keyboard focus into one cell — the path a click on a cell takes, and the
    /// hook tests type through.
    pub fn focus_cell(&self, id: u64, field: RowField, window: &mut Window, cx: &mut App) {
        let Some(row) = self.rows.iter().find(|row| row.id == id) else {
            return;
        };
        let input = match field {
            RowField::Key => &row.key_input,
            RowField::Value => &row.value_input,
            RowField::Description => &row.description_input,
        };
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    /// A cell was edited (or its text was set programmatically). A programmatic
    /// `set_text` echoes [`TextInputEvent::Changed`] with the text the row already holds;
    /// comparing against the stored text turns that echo into a no-op instead of a
    /// spurious change event.
    fn on_cell_changed(
        &mut self,
        id: u64,
        field: RowField,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        let input = match field {
            RowField::Key => &row.key_input,
            RowField::Value => &row.value_input,
            RowField::Description => &row.description_input,
        };
        let text = input.read(cx).text();
        if row.cell_text(field) == text {
            return;
        }
        row.apply_text(field, text);
        cx.emit(KeyValueTableEvent::RowsChanged);
    }

    /// Append one row built from `value`: three stored, tab-reachable cells wired back to
    /// the table by the row's stable id.
    fn push_row(&mut self, value: KeyValue, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.make_row(value, window, cx);
        self.rows.push(row);
    }

    /// Build a row.
    fn make_row(&mut self, value: KeyValue, window: &mut Window, cx: &mut Context<Self>) -> Row {
        let id = self.next_id;
        self.next_id += 1;
        let key_input = self.make_input(RowField::Key, &value.key, window, cx);
        if self.shape == KeyValueTableShape::Derived {
            // The key is owned by whatever derives the row (the URL's `:name` segment).
            key_input.update(cx, |input, cx| input.set_read_only(true, cx));
        }
        let value_input = self.make_input(
            RowField::Value,
            value.value.as_deref().unwrap_or_default(),
            window,
            cx,
        );
        let description_input = self.make_input(
            RowField::Description,
            value.description.as_deref().unwrap_or_default(),
            window,
            cx,
        );
        let subscriptions: Vec<Subscription> = [
            (RowField::Key, &key_input),
            (RowField::Value, &value_input),
            (RowField::Description, &description_input),
        ]
        .into_iter()
        .map(|(field, input)| {
            cx.subscribe_in(input, window, move |table, _input, event, _window, cx| {
                table.on_cell_changed(id, field, event, cx);
            })
        })
        .collect();
        Row {
            id,
            row: value,
            key_input,
            value_input,
            description_input,
            _subscriptions: subscriptions,
        }
    }

    /// One cell: a stored single-line field that reports edits through the row id.
    fn make_input(
        &self,
        field: RowField,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let placeholder = match field {
            RowField::Key => "Key",
            RowField::Value => "Value",
            RowField::Description => "Description",
        };
        let input = cx
            .new(|cx| TextInput::new(window, cx).with_placeholder(SharedString::from(placeholder)));
        // `text_input`'s handle defaults to not being a tab stop; every cell belongs in
        // the tab order (keyboard-first).
        input.read(cx).focus_handle(cx).tab_stop(true);
        input.update(cx, |input, cx| input.set_text(text, cx));
        input
    }

    /// The enabled checkbox of one row.
    fn render_enabled(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let weak = cx.weak_entity();
        Toggle::new(
            ElementId::NamedInteger(SharedString::new_static("enabled"), id),
            SharedString::new_static("Include this row in the request"),
            ToggleState::from(row.row.enabled),
        )
        .on_change(move |state, _window, cx| {
            if weak
                .update(cx, |table, cx| {
                    table.set_enabled(id, state.is_selected(), cx)
                })
                .is_err()
            {
                // Handled: the table was released with its window; no row to enable.
            }
        })
        .into_any_element()
    }

    /// The remove button of one row.
    fn render_remove(&self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let weak = cx.weak_entity();
        IconButton::new(
            ElementId::NamedInteger(SharedString::new_static("remove-row"), id),
            IconName::X,
            Tooltip::text("Remove row"),
        )
        .size(ButtonSize::Default)
        .on_click(move |_event, _window, cx| {
            if weak
                .update(cx, |table, cx| table.remove_row(id, cx))
                .is_err()
            {
                // Handled: the table was released with its window; no row to remove.
            }
        })
        .into_any_element()
    }

    /// The "Add row" button under an editable table.
    fn render_add_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        Button::new(
            ElementId::Name(SharedString::new_static("add-row")),
            SharedString::new_static("Add row"),
        )
        .size(ButtonSize::Default)
        .start_icon(Icon::new(IconName::Plus))
        .tooltip(Tooltip::text("Add a row"))
        .on_click(move |_event, window, cx| {
            if weak
                .update(cx, |table, cx| table.add_row(window, cx))
                .is_err()
            {
                // Handled: the table was released with its window; no table to add to.
            }
        })
    }
}

impl Render for KeyValueTable {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root_id = ElementId::from(&self.focus_handle);
        let mut table = Table::new((root_id.clone(), SharedString::new_static("table")));
        if self.shape == KeyValueTableShape::Editable {
            table =
                table.column(TableColumn::new("Enabled").width(px_to_rems(NARROW_COLUMN_WIDTH_PX)));
        }
        table = table
            .column(TableColumn::new("Key"))
            .column(TableColumn::new("Value"))
            .column(TableColumn::new("Description"));
        if self.shape == KeyValueTableShape::Editable {
            table = table.column(
                TableColumn::new(SharedString::new_static(""))
                    .width(px_to_rems(NARROW_COLUMN_WIDTH_PX)),
            );
        }
        for row in &self.rows {
            let row_id = (root_id.clone(), format!("row-{}", row.id));
            let mut table_row = TableRow::new(row_id);
            if self.shape == KeyValueTableShape::Editable {
                table_row = table_row.cell(self.render_enabled(row, cx));
            }
            table_row = table_row
                .cell(row.key_input.clone())
                .cell(row.value_input.clone())
                .cell(row.description_input.clone());
            if self.shape == KeyValueTableShape::Editable {
                table_row = table_row.cell(self.render_remove(row, cx));
            }
            table = table.row(table_row);
        }
        v_flex()
            .id(root_id)
            .size_full()
            .min_w_0()
            .child(table)
            .children((self.shape == KeyValueTableShape::Editable).then(|| self.render_add_row(cx)))
    }
}
