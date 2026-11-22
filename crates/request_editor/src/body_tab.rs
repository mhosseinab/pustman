//! The Body tab (REQ-HTTP-04/05): the body-mode picker and one editor per M1 body
//! mode — none, form-data (text and file fields with a file dialog and a per-field
//! content type), x-www-form-urlencoded (the reusable [`KeyValueTable`]), raw (a
//! [`TextArea`] the toolbar's language picker names) and binary (a file dialog) — with
//! every file path stored relative to the workspace root when it lives inside it.
//!
//! `Decision:` the tab keeps each mode's state alive across mode switches and hands its
//! owner only the active mode's [`Body`] ([`BodyTab::body`]): switching
//! raw → form-data → raw restores the raw text, and the draft never carries the
//! leftovers of another mode. Each mode also keeps the `extra` its imported body
//! carried, so switching away and back round-trips an import (REQ-MODEL-02).
//!
//! `Decision:` form-data rows are their own row list built from the `ui`
//! [`Table`]/[`Toggle`]/[`IconButton`] primitives, not a second [`KeyValueTable`]: a
//! [`FormField`] row carries a Text/File kind, a file picker and a per-field content
//! type (REQ-HTTP-04) that the table's fixed Key/Value/Description columns cannot
//! express. Urlencoded rows are `KeyValue` rows and use the table as it is.
//!
//! `Decision:` a form field's kind is what the model already encodes — `file` is
//! `Some` on a file field, exactly what the Postman exporter derives `type` from — so
//! there is no second kind flag that could drift from the row. Switching kind swaps
//! `value` and `file`; picking a file for a text row turns it into a file field.
//!
//! `Decision:` [`Body::Unsupported`] (GraphQL and friends) shows read-only and is
//! handed back untouched until the user picks one of the five modes — picking one *is*
//! editing the body, and the unmodelled body is then replaced.
//!
//! `Decision:` the workspace root arrives through [`BodyTab::set_workspace_root`]; who
//! supplies it at runtime is a later wiring concern (nothing calls
//! `HttpClient::set_workspace_root` yet either). Until that lands every picked path is
//! stored absolute.
//!
//! `Decision:` the mode and language pickers compose the `ui` [`Button`] and
//! [`ContextMenu`] in the shared [`floating_menu`] layer — the same shape as the
//! method dropdown (`url_bar`) and the formatter override (`response_viewer`), because
//! `crates/ui` has no popover-menu primitive. A file-dialog failure lands inline under
//! the body controls (AGENTS.md — errors reach the user), never only in a log.
//!
//! `Decision:` the mode picker, the language picker and Beautify share one toolbar row
//! over the editor — mode and language at the left, Beautify at the right (S03/R03,
//! `06_visuals/workbench_blueprint`) — never a row per picker. Beautify reuses the
//! `response_viewer` Pretty formatters rather than a second formatter: a body those
//! cannot parse, and a language with none (plain text, JavaScript), is left exactly as
//! it is and the tab says so inline below the row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::{Body, FormField, HttpRequest, RawLanguage};
use gpui::{
    AnyElement, App, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Subscription, Task, Window,
};
use response_viewer::pretty::Formatter;
use text_input::{TextArea, TextInput, TextInputEvent};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;
use variables::ScopeStack;

use crate::key_value_table::{KeyValueTable, KeyValueTableEvent, KeyValueTableShape};
use crate::variable_highlight;

/// The height of one body control (a [`ButtonSize::Medium`]) in pixels at the default
/// scale: a picker menu is anchored just under it.
const CONTROL_HEIGHT_PX: f32 = 28.;

/// The height the raw editor gets at minimum, in pixels at the default scale: a body
/// editor needs room for more than one line before it starts taking the pane's slack.
/// `pub(crate)` so the layout fix's test measures against the same minimum (S33n).
pub(crate) const RAW_EDITOR_MIN_HEIGHT_PX: f32 = 120.;

/// The width of the enabled, kind and remove columns in pixels at the default scale: a
/// fixed column never squeezes the text columns next to it.
const NARROW_COLUMN_WIDTH_PX: f32 = 48.;

/// The width of the Text/File kind column in pixels at the default scale.
const KIND_COLUMN_WIDTH_PX: f32 = 64.;

/// Test hook: the inline error under the body controls.
pub const BODY_ERROR_SELECTOR: &str = "body-tab-error";

/// Test hooks: the body toolbar row and the controls on it.
pub const BODY_TOOLBAR_SELECTOR: &str = "body-toolbar";
pub const BODY_MODE_SELECTOR: &str = "body-mode-control";
pub const BODY_LANGUAGE_SELECTOR: &str = "body-language-control";
pub const BODY_BEAUTIFY_SELECTOR: &str = "body-beautify";

/// What a [`BodyTab`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyTabEvent {
    /// The tab's contents changed: [`BodyTab::body`] is the new truth for the draft.
    BodyChanged,
}

impl EventEmitter<BodyTabEvent> for BodyTab {}

/// The body modes of REQ-HTTP-04, plus [`BodyMode::Unsupported`] — a body no mode here
/// models (GraphQL, …), shown read-only until one of the five replaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyMode {
    None,
    FormData,
    UrlEncoded,
    Raw,
    Binary,
    Unsupported,
}

impl BodyMode {
    /// The modes the picker lists, in picker order. `Unsupported` is not one of them:
    /// once a mode replaces an unmodelled body there is no way back to it.
    pub const PICKER_MODES: [BodyMode; 5] = [
        BodyMode::None,
        BodyMode::FormData,
        BodyMode::UrlEncoded,
        BodyMode::Raw,
        BodyMode::Binary,
    ];

    /// The mode's name as the picker and the mode button show it.
    pub fn label(self) -> &'static str {
        match self {
            BodyMode::None => "None",
            BodyMode::FormData => "form-data",
            BodyMode::UrlEncoded => "x-www-form-urlencoded",
            BodyMode::Raw => "Raw",
            BodyMode::Binary => "Binary",
            BodyMode::Unsupported => "Unsupported",
        }
    }
}

/// What a form-data field carries: text in `value`, or a file at `file` (REQ-HTTP-04).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFieldKind {
    Text,
    File,
}

/// Which file a file dialog fills (REQ-HTTP-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTarget {
    /// The binary body's file.
    Binary,
    /// The file of one form-data row, by its stable row id.
    FormField(u64),
}

/// Which cell of a form-data row is being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormCell {
    Key,
    Value,
    ContentType,
    Description,
}

/// One form-data row: the [`FormField`] plus the cells that edit it. The cells are
/// stored entities (created once per row) so editing one keeps its focus and caret,
/// and every element id derives from the row's stable id — editing or reordering must
/// not move focus (AGENTS.md › GPUI).
struct FormRow {
    /// The stable id every element id in this row derives from. Ids are never reused,
    /// so an event for a removed row can never land on a different one.
    id: u64,
    field: FormField,
    key_input: Entity<TextInput>,
    value_input: Entity<TextInput>,
    content_type_input: Entity<TextInput>,
    description_input: Entity<TextInput>,
    /// Dropped with the row: its cells' events stop being delivered with it.
    _subscriptions: Vec<Subscription>,
}

/// The Body tab: the mode picker, the active mode's editor, and every mode's state.
pub struct BodyTab {
    /// Where relative file references start (REQ-HTTP-05); `None` stores absolute.
    workspace_root: Option<PathBuf>,
    /// Which mode's editor shows and which mode [`Self::body`] reports.
    mode: BodyMode,
    /// The form-data rows (the mode's data, kept across switches).
    form_rows: Vec<FormRow>,
    form_extra: BTreeMap<String, serde_json::Value>,
    next_row_id: u64,
    /// The urlencoded rows (the mode's data, kept across switches).
    urlencoded_table: Entity<KeyValueTable>,
    urlencoded_extra: BTreeMap<String, serde_json::Value>,
    /// The raw language (the raw text lives in the [`TextArea`], the other half of
    /// this mode's data).
    raw_language: RawLanguage,
    raw_text: Entity<TextArea>,
    raw_extra: BTreeMap<String, serde_json::Value>,
    /// The binary file reference (the mode's data, kept across switches).
    binary_file: Option<String>,
    binary_extra: BTreeMap<String, serde_json::Value>,
    /// The unmodelled body as imported — handed back untouched while it is the mode.
    unsupported: Option<Body>,
    /// Why the last file dialog failed, shown under the body controls.
    error: Option<SharedString>,
    /// The open picker menus and their subscriptions: dropped together when they
    /// dismiss.
    mode_menu: Option<Entity<ContextMenu>>,
    mode_menu_subscription: Option<Subscription>,
    language_menu: Option<Entity<ContextMenu>>,
    language_menu_subscription: Option<Subscription>,
    /// The mode button's focus handle: the menu hands focus back to it when it closes.
    mode_focus_handle: FocusHandle,
    /// The language button's focus handle, likewise.
    language_focus_handle: FocusHandle,
    /// The file-dialog task: replacing it cancels the dialog's answer before it,
    /// because dropping a `Task` cancels it (AGENTS.md › Async).
    pick_task: Option<Task<()>>,
    /// The in-flight Beautify format, likewise: a second click cancels the format
    /// before it.
    beautify_task: Option<Task<()>>,
    /// The scopes the body fields highlight their `{{variables}}` against (REQ-VAR-03).
    scopes: Arc<ScopeStack>,
    _urlencoded_subscription: Subscription,
    _raw_subscription: Subscription,
}

impl BodyTab {
    /// The tab for `request`: every mode's state seeded from its body, so the tab
    /// round-trips what the request already carries until the user edits it.
    pub fn new(request: &HttpRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let urlencoded_table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Editable, cx));
        let _urlencoded_subscription =
            cx.subscribe_in(&urlencoded_table, window, Self::on_urlencoded_event);
        let raw_text = cx.new(|cx| TextArea::new(window, cx));
        let _raw_subscription = cx.subscribe_in(&raw_text, window, Self::on_raw_event);
        let mut tab = Self {
            workspace_root: None,
            mode: BodyMode::None,
            form_rows: Vec::new(),
            form_extra: BTreeMap::new(),
            next_row_id: 0,
            urlencoded_table,
            urlencoded_extra: BTreeMap::new(),
            raw_language: RawLanguage::default(),
            raw_text,
            raw_extra: BTreeMap::new(),
            binary_file: None,
            binary_extra: BTreeMap::new(),
            unsupported: None,
            error: None,
            mode_menu: None,
            mode_menu_subscription: None,
            language_menu: None,
            language_menu_subscription: None,
            mode_focus_handle: cx.focus_handle().tab_stop(true),
            language_focus_handle: cx.focus_handle().tab_stop(true),
            pick_task: None,
            beautify_task: None,
            scopes: Arc::new(ScopeStack::new()),
            _urlencoded_subscription,
            _raw_subscription,
        };
        tab.seed(&request.body, window, cx);
        tab.refresh_highlights(cx);
        tab
    }

    /// Where relative file references start (REQ-HTTP-05). `None` — the state until
    /// the workspace wiring lands — stores every picked path absolute.
    pub fn set_workspace_root(&mut self, root: Option<PathBuf>) {
        self.workspace_root = root;
    }

    /// The mode the tab is editing.
    pub fn mode(&self) -> BodyMode {
        self.mode
    }

    /// The body as the active mode holds it — the new truth for the request draft.
    pub fn body(&self, cx: &App) -> Body {
        match self.mode {
            BodyMode::None => Body::None,
            BodyMode::FormData => Body::FormData {
                fields: self.form_rows.iter().map(|row| row.field.clone()).collect(),
                extra: self.form_extra.clone(),
            },
            BodyMode::UrlEncoded => Body::UrlEncoded {
                fields: self.urlencoded_table.read(cx).rows(),
                extra: self.urlencoded_extra.clone(),
            },
            BodyMode::Raw => Body::Raw {
                language: self.raw_language,
                text: self.raw_text.read(cx).text(),
                extra: self.raw_extra.clone(),
            },
            BodyMode::Binary => Body::Binary {
                file: self.binary_file.clone().unwrap_or_default(),
                extra: self.binary_extra.clone(),
            },
            BodyMode::Unsupported => self.unsupported.clone().unwrap_or(Body::None),
        }
    }

    /// Switch to `mode` — the picker's entries run here. The state of the mode left
    /// behind is kept; the draft gets only the mode now active.
    pub fn set_mode(&mut self, mode: BodyMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.error = None;
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// The raw editor — what tests and later steps reach into.
    pub fn raw_text_area(&self) -> &Entity<TextArea> {
        &self.raw_text
    }

    /// The urlencoded table.
    pub fn urlencoded_table(&self) -> &Entity<KeyValueTable> {
        &self.urlencoded_table
    }

    /// Store the scopes this tab's fields highlight against and re-highlight them
    /// (REQ-VAR-03).
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.refresh_highlights(cx);
    }

    /// Re-highlight everything this tab edits against the current scopes. The
    /// description cells stay plain — the same "never sent" rule
    /// [`variable_highlight::apply_to_table`] applies to key/value rows.
    fn refresh_highlights(&self, cx: &mut Context<Self>) {
        variable_highlight::apply_to_area(&self.raw_text, &self.scopes, cx);
        for row in &self.form_rows {
            variable_highlight::apply_to_input(&row.key_input, &self.scopes, cx);
            variable_highlight::apply_to_input(&row.value_input, &self.scopes, cx);
            variable_highlight::apply_to_input(&row.content_type_input, &self.scopes, cx);
        }
        variable_highlight::apply_to_table(&self.urlencoded_table, &self.scopes, cx);
    }

    /// The raw body's language (REQ-HTTP-04) — part of the body, so the draft follows.
    pub fn set_raw_language(&mut self, language: RawLanguage, cx: &mut Context<Self>) {
        if self.raw_language == language {
            return;
        }
        self.raw_language = language;
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Append an empty form-data field of `kind` and put the caret in its key cell.
    /// `Decision:` this row is body content the moment it exists (the model has no
    /// "kind" of its own — an empty file field is `file: Some("")`), so it announces
    /// itself, unlike [`KeyValueTable::add_row`] whose empty row says nothing.
    pub fn add_form_field(
        &mut self,
        kind: FormFieldKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        let (value, file) = match kind {
            FormFieldKind::Text => (None, None),
            FormFieldKind::File => (None, Some(String::new())),
        };
        let row = self.make_form_row(
            FormField {
                key: String::new(),
                value,
                file,
                content_type: None,
                description: None,
                enabled: true,
                extra: BTreeMap::new(),
            },
            window,
            cx,
        );
        let id = row.id;
        self.form_rows.push(row);
        self.focus_form_cell(id, FormCell::Key, window, cx);
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
        id
    }

    /// Remove the form-data row with `id`.
    pub fn remove_form_field(&mut self, id: u64, cx: &mut Context<Self>) {
        self.form_rows.retain(|row| row.id != id);
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Flip a form-data row's enabled flag — the checkbox in its first cell.
    pub fn set_form_field_enabled(&mut self, id: u64, enabled: bool, cx: &mut Context<Self>) {
        let Some(row) = self.form_rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        if row.field.enabled == enabled {
            return;
        }
        row.field.enabled = enabled;
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Switch a form-data row between text and file: what one side carried becomes the
    /// other side's starting content (Postman's Text/File switch keeps the cell too).
    pub fn set_form_field_kind(&mut self, id: u64, kind: FormFieldKind, cx: &mut Context<Self>) {
        let Some(row) = self.form_rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        match kind {
            FormFieldKind::File if row.field.file.is_none() => {
                row.field.file = Some(row.field.value.take().unwrap_or_default());
            }
            FormFieldKind::Text if row.field.file.is_some() => {
                let text = row.field.file.take().unwrap_or_default();
                row.field.value = (!text.is_empty()).then_some(text);
            }
            _ => return,
        }
        let value_text = row.field.value.clone().unwrap_or_default();
        row.value_input
            .update(cx, |input, cx| input.set_text(&value_text, cx));
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Open the platform file dialog and fill `target` with what it returns
    /// (REQ-HTTP-05). A dialog the user dismisses changes nothing; a dialog that
    /// cannot open says so inline (AGENTS.md — errors reach the user).
    pub fn choose_file(
        &mut self,
        target: FileTarget,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::new_static("Choose a body file")),
        });
        self.error = None;
        self.pick_task = Some(cx.spawn(async move |this, cx| {
            let picked = match receiver.await {
                Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
                Ok(Err(error)) => {
                    let message = format!("Could not open the file dialog — {error}");
                    if this
                        .update(cx, |tab, cx| tab.show_error(message, cx))
                        .is_err()
                    {
                        // Handled: the tab was released with its window; no error to show.
                    }
                    return;
                }
                Err(_canceled) => {
                    let message = String::from("The file dialog did not return a file name");
                    if this
                        .update(cx, |tab, cx| tab.show_error(message, cx))
                        .is_err()
                    {
                        // Handled: the tab was released with its window; no error to show.
                    }
                    return;
                }
            };
            let Some(picked) = picked else {
                return;
            };
            if this
                .update(cx, |tab, cx| tab.apply_picked_file(target, &picked, cx))
                .is_err()
            {
                // Handled: the tab was released with its window; no file to fill.
            }
        }));
    }

    /// Store the path the file dialog returned where [`FileTarget`] names it, in stored
    /// form (REQ-HTTP-05) — the one place a picked path enters the body.
    pub fn apply_picked_file(&mut self, target: FileTarget, picked: &Path, cx: &mut Context<Self>) {
        let stored = stored_file_path(self.workspace_root.as_deref(), picked);
        match target {
            FileTarget::Binary => self.binary_file = Some(stored),
            FileTarget::FormField(id) => {
                if let Some(row) = self.form_rows.iter_mut().find(|row| row.id == id) {
                    row.field.file = Some(stored);
                    row.field.value = None;
                    row.value_input
                        .update(cx, |input, cx| input.set_text("", cx));
                }
            }
        }
        self.error = None;
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Seed every mode's state from `body` — the open path (a later re-seed is a
    /// draft-replacement concern of the owner, not of this tab).
    fn seed(&mut self, body: &Body, window: &mut Window, cx: &mut Context<Self>) {
        match body {
            Body::None => self.mode = BodyMode::None,
            Body::FormData { fields, extra } => {
                self.mode = BodyMode::FormData;
                for field in fields {
                    let row = self.make_form_row(field.clone(), window, cx);
                    self.form_rows.push(row);
                }
                self.form_extra = extra.clone();
            }
            Body::UrlEncoded { fields, extra } => {
                self.mode = BodyMode::UrlEncoded;
                self.urlencoded_table
                    .update(cx, |table, cx| table.set_rows(fields.clone(), window, cx));
                self.urlencoded_extra = extra.clone();
            }
            Body::Raw {
                language,
                text,
                extra,
            } => {
                self.mode = BodyMode::Raw;
                self.raw_language = *language;
                self.raw_text.update(cx, |area, cx| area.set_text(text, cx));
                self.raw_extra = extra.clone();
            }
            Body::Binary { file, extra } => {
                self.mode = BodyMode::Binary;
                self.binary_file = Some(file.clone());
                self.binary_extra = extra.clone();
            }
            Body::Unsupported(_) => {
                self.mode = BodyMode::Unsupported;
                self.unsupported = Some(body.clone());
            }
        }
    }

    /// Show `message` under the body controls — the file dialog's failure path.
    fn show_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(SharedString::from(message));
        cx.notify();
    }

    /// Move keyboard focus into one form-data row cell.
    fn focus_form_cell(&self, id: u64, cell: FormCell, window: &mut Window, cx: &mut App) {
        let Some(row) = self.form_rows.iter().find(|row| row.id == id) else {
            return;
        };
        let input = match cell {
            FormCell::Key => &row.key_input,
            FormCell::Value => &row.value_input,
            FormCell::ContentType => &row.content_type_input,
            FormCell::Description => &row.description_input,
        };
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    /// A form-data cell was edited (or its text was set programmatically). A
    /// programmatic `set_text` echoes [`TextInputEvent::Changed`] with the text the row
    /// already holds; comparing against the stored text turns that echo into a no-op
    /// instead of a spurious change event.
    fn on_form_cell_changed(
        &mut self,
        id: u64,
        cell: FormCell,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        self.refresh_highlights(cx);
        let Some(row) = self.form_rows.iter_mut().find(|row| row.id == id) else {
            return;
        };
        let input = match cell {
            FormCell::Key => &row.key_input,
            FormCell::Value => &row.value_input,
            FormCell::ContentType => &row.content_type_input,
            FormCell::Description => &row.description_input,
        };
        let text = input.read(cx).text();
        if form_cell_text(&row.field, cell) == text {
            return;
        }
        apply_form_cell(&mut row.field, cell, text);
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// The urlencoded rows changed: the draft follows through the owner.
    fn on_urlencoded_event(
        &mut self,
        _table: &Entity<KeyValueTable>,
        event: &KeyValueTableEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != KeyValueTableEvent::RowsChanged {
            return;
        }
        self.refresh_highlights(cx);
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// The raw text changed: it is half of the raw body, so the draft follows.
    fn on_raw_event(
        &mut self,
        _area: &Entity<TextArea>,
        event: &TextInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        self.refresh_highlights(cx);
        cx.emit(BodyTabEvent::BodyChanged);
        cx.notify();
    }

    /// Append one form-data row built from `field`: four stored, tab-reachable cells
    /// wired back to the tab by the row's stable id.
    fn make_form_row(
        &mut self,
        field: FormField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FormRow {
        let id = self.next_row_id;
        self.next_row_id += 1;
        let key_input = self.make_input("Key", &field.key, window, cx);
        let value_input = self.make_input(
            "Value",
            field.value.as_deref().unwrap_or_default(),
            window,
            cx,
        );
        let content_type_input = self.make_input(
            "Content-Type",
            field.content_type.as_deref().unwrap_or_default(),
            window,
            cx,
        );
        let description_input = self.make_input(
            "Description",
            field.description.as_deref().unwrap_or_default(),
            window,
            cx,
        );
        let subscriptions: Vec<Subscription> = [
            (FormCell::Key, &key_input),
            (FormCell::Value, &value_input),
            (FormCell::ContentType, &content_type_input),
            (FormCell::Description, &description_input),
        ]
        .into_iter()
        .map(|(cell, input)| {
            cx.subscribe_in(input, window, move |tab, _input, event, _window, cx| {
                tab.on_form_cell_changed(id, cell, event, cx);
            })
        })
        .collect();
        FormRow {
            id,
            field,
            key_input,
            value_input,
            content_type_input,
            description_input,
            _subscriptions: subscriptions,
        }
    }

    /// One cell: a stored single-line field that reports edits through the row id.
    fn make_input(
        &self,
        placeholder: &'static str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let input = cx
            .new(|cx| TextInput::new(window, cx).with_placeholder(SharedString::from(placeholder)));
        // `text_input`'s handle defaults to not being a tab stop; every cell belongs in
        // the tab order (keyboard-first).
        input.read(cx).focus_handle(cx).tab_stop(true);
        input.update(cx, |input, cx| input.set_text(text, cx));
        input
    }

    /// Open the body-mode menu under the mode button.
    fn open_mode_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode_menu.is_some() {
            return;
        }
        let picker = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for mode in BodyMode::PICKER_MODES {
                let picker = picker.clone();
                menu = menu.entry(mode.label(), move |_event, _window, cx| {
                    if picker.update(cx, |tab, cx| tab.set_mode(mode, cx)).is_err() {
                        // The tab was dropped while the menu was open; there is no
                        // body left to edit.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_mode_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.mode_menu = Some(menu);
        self.mode_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The mode menu is done — an entry ran or Escape backed out.
    fn on_mode_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.mode_menu = None;
        self.mode_menu_subscription = None;
        window.focus(&self.mode_focus_handle, cx);
        cx.notify();
    }

    /// Open the raw-language menu under the language button.
    fn open_language_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.language_menu.is_some() {
            return;
        }
        let picker = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx);
            for language in RAW_LANGUAGES {
                let picker = picker.clone();
                menu = menu.entry(language_label(language), move |_event, _window, cx| {
                    if picker
                        .update(cx, |tab, cx| tab.set_raw_language(language, cx))
                        .is_err()
                    {
                        // The tab was dropped while the menu was open; there is no
                        // body left to edit.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_language_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.language_menu = Some(menu);
        self.language_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The language menu is done.
    fn on_language_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.language_menu = None;
        self.language_menu_subscription = None;
        window.focus(&self.language_focus_handle, cx);
        cx.notify();
    }

    /// Re-format the raw body (Beautify). Formatting is heavy work, so it runs on a
    /// background task the way `response_viewer`'s Pretty view runs it — the body
    /// stays as it is until the formatted text lands, and a second click cancels the
    /// format before it. A body the formatters cannot parse — and a language that has
    /// no formatter at all — is left exactly as it is while the tab says why inline
    /// under the controls: a button that silently does nothing is not an answer
    /// (AGENTS.md — errors reach the user).
    fn beautify(&mut self, cx: &mut Context<Self>) {
        let Some(formatter) = formatter_for(self.raw_language) else {
            self.error = Some(SharedString::new_static(
                "Beautify formats JSON, XML and HTML bodies — choose one of them as the \
                 body's language above.",
            ));
            cx.notify();
            return;
        };
        let text = self.raw_text.read(cx).text();
        self.beautify_task = Some(cx.spawn(async move |this, cx| {
            let to_format = text.clone();
            let outcome = cx
                .background_spawn(async move { formatter.format(to_format.as_bytes()) })
                .await;
            if let Err(_released) = this.update(cx, |tab, cx| {
                // The body changed while it was formatting: the user's typing is the
                // truth and the formatted text would clobber it.
                if tab.raw_text.read(cx).text() != text {
                    return;
                }
                match outcome {
                    Ok(formatted) => {
                        tab.error = None;
                        if formatted != text {
                            tab.raw_text
                                .update(cx, |area, cx| area.set_text(&formatted, cx));
                        }
                    }
                    Err(error) => {
                        tab.error = Some(SharedString::from(format!(
                            "Beautify could not format this body: {error}."
                        )));
                    }
                }
                cx.notify();
            }) {
                // Handled: the tab went away with its window; there is nobody left to
                // show the formatted body to.
            }
        }));
    }

    /// The one toolbar row over the body editor: the mode picker at the left, and —
    /// over a raw body — the language picker beside it with Beautify ending the row at
    /// the right.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let raw = self.mode == BodyMode::Raw;
        h_flex()
            .id(ElementId::Name(SharedString::new_static("body-toolbar")))
            .debug_selector(|| BODY_TOOLBAR_SELECTOR.to_string())
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .min_w_0()
            .child(self.render_mode_control(cx))
            .children(raw.then(|| self.render_language_control(cx)))
            .children(raw.then(|| div().flex_1().min_w_0()))
            .children(raw.then(|| self.render_beautify_control(cx)))
            .into_any_element()
    }

    /// The Beautify button ending the toolbar row.
    fn render_beautify_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .debug_selector(|| BODY_BEAUTIFY_SELECTOR.to_string())
            .child(
                Button::new(
                    ElementId::Name(SharedString::new_static("beautify-body")),
                    SharedString::new_static("Beautify"),
                )
                .size(ButtonSize::Medium)
                .tooltip(Tooltip::text("Format the body with even indentation"))
                .on_click(cx.listener(|this, _event, _window, cx| this.beautify(cx))),
            )
            .into_any_element()
    }

    /// The mode control: the dropdown trigger. The menu floats under it.
    fn render_mode_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .relative()
            .debug_selector(|| BODY_MODE_SELECTOR.to_string())
            .child(
                Button::new(
                    ElementId::from(&self.mode_focus_handle),
                    self.mode_button_label(),
                )
                .size(ButtonSize::Medium)
                .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                .tooltip(Tooltip::text("Choose the body mode"))
                .focus_handle(self.mode_focus_handle.clone())
                .on_click(cx.listener(|this, _event, window, cx| this.open_mode_menu(window, cx))),
            )
            .children(floating_menu(
                &self.mode_menu,
                px_to_rems(CONTROL_HEIGHT_PX),
                cx,
            ))
            .into_any_element()
    }

    /// The language control over the raw editor: the dropdown trigger, likewise.
    fn render_language_control(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .relative()
            .debug_selector(|| BODY_LANGUAGE_SELECTOR.to_string())
            .child(
                Button::new(
                    ElementId::from(&self.language_focus_handle),
                    SharedString::from(language_label(self.raw_language)),
                )
                .size(ButtonSize::Default)
                .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
                .tooltip(Tooltip::text("Choose the raw body's language"))
                .focus_handle(self.language_focus_handle.clone())
                .on_click(
                    cx.listener(|this, _event, window, cx| this.open_language_menu(window, cx)),
                ),
            )
            .children(floating_menu(
                &self.language_menu,
                px_to_rems(CONTROL_HEIGHT_PX),
                cx,
            ))
            .into_any_element()
    }

    /// The mode button's label: the active mode's name, or the imported body's own
    /// mode name for a body no mode here models.
    fn mode_button_label(&self) -> SharedString {
        match self.mode {
            BodyMode::Unsupported => {
                match self.unsupported.as_ref().and_then(unsupported_mode_name) {
                    Some(name) => SharedString::from(format!("{name} (kept as imported)")),
                    None => SharedString::from(BodyMode::Unsupported.label()),
                }
            }
            mode => SharedString::from(mode.label()),
        }
    }

    /// The active mode's editor.
    fn render_mode_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.mode {
            BodyMode::None => Label::new(SharedString::new_static(
                "This request is sent without a body.",
            ))
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
            BodyMode::FormData => self.render_form_data(cx),
            BodyMode::UrlEncoded => self.urlencoded_table.clone().into_any_element(),
            BodyMode::Raw => self.render_raw(cx),
            BodyMode::Binary => self.render_binary(cx),
            BodyMode::Unsupported => Label::new(SharedString::new_static(
                "This body uses a mode Pustman does not edit yet (for example GraphQL). \
                 It is kept exactly as imported until you choose a mode above.",
            ))
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
        }
    }

    /// The raw body: the [`TextArea`] the toolbar's language picker and Beautify act
    /// on.
    fn render_raw(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(ElementId::Name(SharedString::new_static("raw-body")))
            .flex_1()
            .min_h(px_to_rems(RAW_EDITOR_MIN_HEIGHT_PX))
            .min_w_0()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_sm()
            .child(self.raw_text.clone())
            .into_any_element()
    }

    /// The binary body: the stored file and the dialog button beside it.
    fn render_binary(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let path = self.binary_file.clone().unwrap_or_default();
        h_flex()
            .id(ElementId::Name(SharedString::new_static("binary-body")))
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .min_w_0()
            .child(file_label(
                ElementId::Name(SharedString::new_static("binary-file")),
                &path,
            ))
            .child(
                Button::new(
                    ElementId::Name(SharedString::new_static("choose-binary-file")),
                    SharedString::new_static("Choose File…"),
                )
                .size(ButtonSize::Default)
                .start_icon(Icon::new(IconName::File))
                .tooltip(Tooltip::text("Choose the file this request sends"))
                .on_click(move |_event, window, cx| {
                    if weak
                        .update(cx, |tab, cx| {
                            tab.choose_file(FileTarget::Binary, window, cx)
                        })
                        .is_err()
                    {
                        // Handled: the tab was released with its window; no file to pick.
                    }
                }),
            )
            .into_any_element()
    }

    /// The form-data rows and the buttons that add more.
    fn render_form_data(&self, cx: &mut Context<Self>) -> AnyElement {
        let root_id = ElementId::Name(SharedString::new_static("form-data"));
        let mut table = Table::new((root_id.clone(), SharedString::new_static("table")))
            .column(TableColumn::new("Use").width(px_to_rems(NARROW_COLUMN_WIDTH_PX)))
            .column(TableColumn::new("Type").width(px_to_rems(KIND_COLUMN_WIDTH_PX)))
            .column(TableColumn::new("Key"))
            .column(TableColumn::new("Value"))
            .column(TableColumn::new("Content Type"))
            .column(TableColumn::new("Description"))
            .column(
                TableColumn::new(SharedString::new_static(""))
                    .width(px_to_rems(NARROW_COLUMN_WIDTH_PX)),
            );
        for row in &self.form_rows {
            table = table.row(self.render_form_row(row, root_id.clone(), cx));
        }
        v_flex()
            .id(root_id)
            .size_full()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(table)
            .child(
                h_flex()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .child(self.render_add_field_button(
                        ElementId::Name(SharedString::new_static("add-text-field")),
                        SharedString::new_static("Add text field"),
                        FormFieldKind::Text,
                        cx,
                    ))
                    .child(self.render_add_field_button(
                        ElementId::Name(SharedString::new_static("add-file-field")),
                        SharedString::new_static("Add file field"),
                        FormFieldKind::File,
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// One form-data row: the enabled box, the Text/File switch, the four cells and
    /// the remove button.
    fn render_form_row(
        &self,
        row: &FormRow,
        root_id: ElementId,
        cx: &mut Context<Self>,
    ) -> TableRow {
        let id = row.id;
        TableRow::new((root_id, format!("row-{id}")))
            .cell(self.render_form_enabled(row, cx))
            .cell(self.render_form_kind(row, cx))
            .cell(row.key_input.clone())
            .cell(self.render_form_value(row, cx))
            .cell(row.content_type_input.clone())
            .cell(row.description_input.clone())
            .cell(self.render_form_remove(row, cx))
    }

    /// The enabled checkbox of one form-data row. Its label is the accessible name
    /// only: the "Use" column header is the row's visible meaning, and painted
    /// label text would run over the Key cell beside it (S33n).
    fn render_form_enabled(&self, row: &FormRow, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let weak = cx.weak_entity();
        Toggle::new(
            ElementId::NamedInteger(SharedString::new_static("field-enabled"), id),
            SharedString::new_static("Include this field in the request"),
            ToggleState::from(row.field.enabled),
        )
        .label_hidden()
        .on_change(move |state, _window, cx| {
            if weak
                .update(cx, |tab, cx| {
                    tab.set_form_field_enabled(id, state.is_selected(), cx)
                })
                .is_err()
            {
                // Handled: the tab was released with its window; no field to enable.
            }
        })
        .into_any_element()
    }

    /// The Text/File switch of one form-data row: it shows the kind it will leave, so
    /// clicking it is always the switch to the other one.
    fn render_form_kind(&self, row: &FormRow, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let (label, switch_to, tip) = match row.field.file.is_some() {
            true => (
                "File",
                FormFieldKind::Text,
                "Switch this field to a text field",
            ),
            false => (
                "Text",
                FormFieldKind::File,
                "Switch this field to a file field",
            ),
        };
        let weak = cx.weak_entity();
        Button::new(
            ElementId::NamedInteger(SharedString::new_static("field-kind"), id),
            SharedString::from(label),
        )
        .size(ButtonSize::Default)
        .tooltip(Tooltip::text(tip))
        .on_click(move |_event, _window, cx| {
            if weak
                .update(cx, |tab, cx| tab.set_form_field_kind(id, switch_to, cx))
                .is_err()
            {
                // Handled: the tab was released with its window; no field to switch.
            }
        })
        .into_any_element()
    }

    /// The value cell of one form-data row: the text field, or the stored file path
    /// with the dialog button beside it.
    fn render_form_value(&self, row: &FormRow, cx: &mut Context<Self>) -> AnyElement {
        if row.field.file.is_none() {
            return row.value_input.clone().into_any_element();
        }
        let id = row.id;
        let path = row.field.file.clone().unwrap_or_default();
        let weak = cx.weak_entity();
        h_flex()
            .id(ElementId::NamedInteger(
                SharedString::new_static("file-cell"),
                id,
            ))
            .items_center()
            .gap(DynamicSpacing::Base02.rems(cx))
            .min_w_0()
            .child(file_label(
                ElementId::NamedInteger(SharedString::new_static("field-file"), id),
                &path,
            ))
            .child(
                Button::new(
                    ElementId::NamedInteger(SharedString::new_static("choose-file"), id),
                    SharedString::new_static("Choose File…"),
                )
                .size(ButtonSize::Default)
                .start_icon(Icon::new(IconName::File))
                .tooltip(Tooltip::text("Choose the file this field sends"))
                .on_click(move |_event, window, cx| {
                    if weak
                        .update(cx, |tab, cx| {
                            tab.choose_file(FileTarget::FormField(id), window, cx)
                        })
                        .is_err()
                    {
                        // Handled: the tab was released with its window; no field to fill.
                    }
                }),
            )
            .into_any_element()
    }

    /// The remove button of one form-data row.
    fn render_form_remove(&self, row: &FormRow, cx: &mut Context<Self>) -> AnyElement {
        let id = row.id;
        let weak = cx.weak_entity();
        IconButton::new(
            ElementId::NamedInteger(SharedString::new_static("remove-field"), id),
            IconName::X,
            Tooltip::text("Remove field"),
        )
        .size(ButtonSize::Default)
        .on_click(move |_event, _window, cx| {
            if weak
                .update(cx, |tab, cx| tab.remove_form_field(id, cx))
                .is_err()
            {
                // Handled: the tab was released with its window; no field to remove.
            }
        })
        .into_any_element()
    }

    /// One "Add field" button.
    fn render_add_field_button(
        &self,
        id: ElementId,
        label: SharedString,
        kind: FormFieldKind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.weak_entity();
        Button::new(id, label)
            .size(ButtonSize::Default)
            .start_icon(Icon::new(IconName::Plus))
            .tooltip(Tooltip::text("Add a field to the form body"))
            .on_click(move |_event, window, cx| {
                if weak
                    .update(cx, |tab, cx| {
                        tab.add_form_field(kind, window, cx);
                    })
                    .is_err()
                {
                    // Handled: the tab was released with its window; no field to add.
                }
            })
    }

    /// The inline error under the body controls: what the file dialog failed at.
    fn render_error(&self, message: SharedString) -> impl IntoElement {
        h_flex()
            .id(ElementId::Name(SharedString::new_static("body-tab-error")))
            .debug_selector(|| BODY_ERROR_SELECTOR.to_string())
            .child(
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Error),
            )
    }
}

impl Render for BodyTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::Name(SharedString::new_static("body-tab")))
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(self.render_toolbar(cx))
            .child(self.render_mode_content(cx))
            .children(self.error.clone().map(|message| self.render_error(message)))
    }
}

/// The raw languages the picker lists, in picker order.
const RAW_LANGUAGES: [RawLanguage; 5] = [
    RawLanguage::Text,
    RawLanguage::Json,
    RawLanguage::Xml,
    RawLanguage::Html,
    RawLanguage::JavaScript,
];

/// The language's name as the picker and the language button show it.
fn language_label(language: RawLanguage) -> &'static str {
    match language {
        RawLanguage::Text => "Text",
        RawLanguage::Json => "JSON",
        RawLanguage::Xml => "XML",
        RawLanguage::Html => "HTML",
        RawLanguage::JavaScript => "JavaScript",
    }
}

/// The pretty-printer a raw language maps onto: JSON, XML and HTML have one (the
/// `response_viewer` Pretty formatters), plain text and JavaScript have nothing to
/// format.
fn formatter_for(language: RawLanguage) -> Option<Formatter> {
    match language {
        RawLanguage::Json => Some(Formatter::Json),
        RawLanguage::Xml => Some(Formatter::Xml),
        RawLanguage::Html => Some(Formatter::Html),
        RawLanguage::Text | RawLanguage::JavaScript => None,
    }
}

/// The `mode` tag of an unmodelled body ("graphql", …) — what the mode button names it.
fn unsupported_mode_name(body: &Body) -> Option<&str> {
    let Body::Unsupported(value) = body else {
        return None;
    };
    value.get("mode").and_then(|tag| tag.as_str())
}

/// The text a form-data row's cell shows.
fn form_cell_text(field: &FormField, cell: FormCell) -> String {
    match cell {
        FormCell::Key => field.key.clone(),
        FormCell::Value => field.value.clone().unwrap_or_default(),
        FormCell::ContentType => field.content_type.clone().unwrap_or_default(),
        FormCell::Description => field.description.clone().unwrap_or_default(),
    }
}

/// Apply edited cell text. An empty optional cell is `None` again — the model stores
/// absence, not empty strings.
fn apply_form_cell(field: &mut FormField, cell: FormCell, text: String) {
    match cell {
        FormCell::Key => field.key = text,
        FormCell::Value => field.value = (!text.is_empty()).then_some(text),
        FormCell::ContentType => field.content_type = (!text.is_empty()).then_some(text),
        FormCell::Description => field.description = (!text.is_empty()).then_some(text),
    }
}

/// How a picked file is stored in the body (REQ-HTTP-05): relative to the workspace
/// root with `/` separators when the file lives inside it, and exactly as picked when
/// it does not (or when there is no root). The send resolves the reference the other
/// way (`http_engine::resolve_file_ref`).
fn stored_file_path(root: Option<&Path>, picked: &Path) -> String {
    let Some(relative) = root.and_then(|root| picked.strip_prefix(root).ok()) else {
        return picked.to_string_lossy().into_owned();
    };
    if relative.as_os_str().is_empty() {
        return picked.to_string_lossy().into_owned();
    }
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// A stored file reference as the body controls show it: the path, or "No file chosen"
/// while a file field is still empty.
fn file_label(id: impl Into<ElementId>, path: &str) -> impl IntoElement {
    let chosen = !path.is_empty();
    let shown = if chosen {
        path.to_string()
    } else {
        String::from("No file chosen")
    };
    div()
        .id(id)
        .min_w_0()
        .child(
            Label::new(SharedString::from(shown.clone()))
                .size(LabelSize::Small)
                .color(if chosen { Color::Default } else { Color::Muted })
                .truncate(),
        )
        .tooltip(Tooltip::text(shown).builder())
}

#[cfg(test)]
#[path = "body_tab_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "body_tab_toolbar_tests.rs"]
mod toolbar_tests;
