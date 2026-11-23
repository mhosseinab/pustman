//! The variables editor (PRD REQ-VAR-SHARE-01/02): one modal table over one scope's
//! variables — an environment, a collection's variables or the workspace's globals —
//! showing key, shared value, local value, kind (Default/Secret), enabled and
//! description. Saving writes the shared values to the scope's own file through
//! [`workspace_store::save_item`] and the local values to `storage`'s `local_values`
//! table, both on one background task; a save that fails is a toast on the workspace and
//! leaves the editor open on the rows it failed on.
//!
//! A secret row renders **no shared-value field and no local-value field** (REQ-VAR-SHARE-02):
//! a secret never has a shared value — the file records its key, kind and description
//! only, and [`workspace_store`] strips a smuggled value again on the way out — and its
//! value belongs to the OS keychain, never to the plain local store. A value a hand edit
//! or an import leaves on a secret is dropped when the rows are built: it reaches neither
//! a field nor the display. The keychain side of a secret's value is a later step's work.
//!
//! `Decision:` the surface is the workspace's modal ([`Workspace::toggle_modal`]), the
//! same shape [`super::quick_look`] and `response_viewer::cookie_dialog` open theirs
//! with, not a pane item: the step allows either, and the modal layer already owns
//! placement, focus trapping and Escape dismissal, while a pane item would need
//! workspace-crate support this step's Edits set does not reach.
//!
//! `Decision:` [`EnvEditorInputs`] is an app-global the app's wire installs with
//! [`install_inputs`] — this crate's own [`super::selector::SelectorInputs`] shape for
//! the same "the top crate owns the inputs" problem. The wire names the scope to edit,
//! the folder its file lives in and the local store; `env_editor::Open` opens that scope
//! and an editor opened before anything is installed says so in an error toast instead
//! of silently editing nothing.
//!
//! `Decision:` the globals' shared values live in `pustman.json`, in a `variables` field
//! carried by [`api_model::Workspace::extra`]'s verbatim round-trip of unknown fields
//! (REQ-MODEL-02) and written by the same [`workspace_store::save_item`] every other item
//! uses. The workspace format (PRD §6.1) gives globals no file of their own and this
//! step's Edits set may not extend `workspace_store`, so the workspace file is the one
//! place a shared, team-visible globals list can live without inventing a file no reader
//! knows; a later workspace-format step can move it and this module changes in one place.
//!
//! `Decision:` the scope id local values are keyed by is the same string
//! [`super::quick_look`] uses — the environment's or collection's [`api_model::ItemId`]
//! rendering, and [`GLOBALS_SCOPE_ID`] for the globals. It is re-stated here rather than
//! shared because that module's constant is private to it and this step's Edits set may
//! not touch it; the two must not drift apart.
//!
//! `Decision:` flipping a row to Secret destroys the text in its value fields. The text
//! cannot survive the flip: a secret has no shared value and no local value, so keeping
//! it around would only wait to be written back if the row were flipped again
//! (REQ-VAR-SHARE-02). Flipping back gives empty fields.
//!
//! `Decision:` Save writes the whole scope as the editor shows it — every variable to the
//! file, each row's local value to storage and a cleared local field as a deletion
//! ("reset to shared") — and then closes the editor. The save is detached rather than
//! stored in a field (closing the editor right after Save must not cancel a write that
//! has not run — the file would keep the old values while the user believes they were
//! saved), which is only sound with the task carrying its own error report: the other
//! half of this contract is the toast, and an editor that is still open shows the same
//! toast and keeps every edit (the `cookie_dialog` shape).
//!
//! `Decision:` the actions are declared here (`env_editor::Open`, `env_editor::Save`)
//! rather than in `pustman_actions`: this step's Edits set reaches neither that crate nor
//! `assets/keymaps/`, and the `gpui-actions-keymaps` skill's own template declares
//! actions in the feature crate. Save is dispatched by its button and by any key binding
//! over the editor's [`EDITOR_CONTEXT`]; the default binding is a wiring step's line.
//! Rows are edited in place and this step adds no add/remove-row controls.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use api_model::{Variable, VariableKind};
use gpui::{
    Action, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Global,
    Render, WeakEntity, Window,
};
use storage::{LocalStore, LocalValue};
use text_input::TextInput;
use ui::prelude::*;
use workspace::{ModalView, Workspace};
use workspace_store::{Item, ItemBody};

#[cfg(test)]
#[path = "env_editor_tests.rs"]
mod tests;

gpui::actions!(
    env_editor,
    [
        /// Opens the variables editor on the scope the app's wire installed.
        Open,
        /// Saves the edited shared values to the file and the local values to storage.
        Save,
    ]
);

/// The editor's key context: what a key binding names to reach its actions.
const EDITOR_CONTEXT: &str = "EnvEditor";

/// The scope id the globals' local values (and later secrets) are keyed by — the same
/// string [`super::quick_look`] uses (module docs).
const GLOBALS_SCOPE_ID: &str = "globals";

/// The globals block's label — the same string [`super::quick_look`] shows.
const GLOBALS_LABEL: &str = "Globals";

/// The field `pustman.json` carries the globals' shared values in (module docs).
const GLOBALS_VARIABLES_FIELD: &str = "variables";

/// What a cell shows where there is no value to show.
const NO_VALUE_LABEL: &str = "—";

/// What an empty scope shows instead of a table with no rows.
const NO_VARIABLES_LABEL: &str = "No variables yet.";

/// Why a secret row's shared cell has no field (REQ-VAR-SHARE-02).
const SECRET_SHARED_TOOLTIP: &str =
    "A secret never has a shared value — the file records its key and description only.";

/// Why a secret row's local cell has no field: its value is the OS keychain's.
const SECRET_LOCAL_TOOLTIP: &str =
    "A secret's value lives in the OS keychain, never in the local store.";

/// The fields' placeholders — the same "an example, not a value" shape
/// `response_viewer::cookie_form` uses.
const KEY_PLACEHOLDER: &str = "e.g. base_url";
const VALUE_PLACEHOLDER: &str = "e.g. https://api.example.test";
const DESCRIPTION_PLACEHOLDER: &str = "what this variable is for";

/// What a row with an empty key is told before a save is refused (AGENTS.md: errors are
/// actionable).
const KEY_REQUIRED_MESSAGE: &str =
    "Every variable needs a key — fill the empty Key cell before saving.";

/// The prefix and suffix around the key two rows share.
const DUPLICATE_KEY_PREFIX: &str = "Two variables share the key `";
const DUPLICATE_KEY_SUFFIX: &str = "` — keys must be unique before saving.";

/// The one message the opener shows when the app has not installed [`EnvEditorInputs`]
/// (AGENTS.md: errors reach the user, never only a log).
const NOT_WIRED_MESSAGE: &str = "The variables editor cannot save changes — the workspace folder and local store are not connected";

/// What every failed save is shown with, so the toast says what did not happen before it
/// quotes the reason.
const SAVE_FAILURE_PREFIX: &str = "Could not save the variables — ";

/// The dialog's width: room for six columns to read.
const EDITOR_WIDTH_PX: f32 = 1024.;

/// How tall the table grows before a big scope scrolls inside the dialog.
const TABLE_MAX_HEIGHT_PX: f32 = 384.;

/// How wide the "Kind" and "Enabled" columns are: the box and the kind word beside it.
const KIND_COLUMN_PX: f32 = 96.;

/// How wide the "Enabled" column is: one labelled checkbox.
const ENABLED_COLUMN_PX: f32 = 80.;

/// What one editor session cannot know on its own: the scope to edit, the workspace
/// folder its file lives in and the local store its local values go to. See the module
/// docs for the app-global caveat this shape carries.
#[derive(Clone)]
pub struct EnvEditorInputs {
    /// The workspace the variables and their local values belong to.
    pub workspace_id: String,
    /// The workspace folder the scope's file is saved into.
    pub workspace_root: PathBuf,
    /// The scope to edit.
    pub target: EditTarget,
    /// The scope's non-secret local values (PRD REQ-VAR-SHARE-01), matched by key.
    pub local_values: Vec<LocalValue>,
    /// The app's local store the local values are saved to.
    pub store: Arc<Mutex<LocalStore>>,
}

impl Global for EnvEditorInputs {}

/// Install the inputs the opener runs with; the next `env_editor::Open` uses them.
pub fn install_inputs(cx: &mut App, inputs: EnvEditorInputs) {
    cx.set_global(inputs);
}

/// Why an item cannot be edited as a variables scope, or cannot be written back.
/// Carries no payload on purpose: a parse or serialization error's own text can quote
/// input, and input can hold values (AGENTS.md › Security).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTargetError {
    /// The item is a folder or a request: it holds no variables.
    NotAVariableScope,
    /// The globals' `variables` field is not a variable list.
    GlobalsUnreadable,
    /// The edited variables could not be written back into the file.
    VariablesUnserializable,
}

impl fmt::Display for EditTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAVariableScope => {
                f.write_str("only environments, collections and the globals carry variables")
            }
            Self::GlobalsUnreadable => f.write_str(
                "the `variables` field in pustman.json could not be read as a variable list",
            ),
            Self::VariablesUnserializable => {
                f.write_str("the variables could not be written back into the file")
            }
        }
    }
}

impl std::error::Error for EditTargetError {}

/// The scope one editor session edits: its variables as the file holds them, and the
/// workspace item whose file their shared values are saved to.
#[derive(Clone)]
pub struct EditTarget {
    /// The scope's name, in the dialog's title.
    label: String,
    /// The scope id the scope's local values are keyed by (module docs).
    scope_id: String,
    /// The variables as the file holds them.
    variables: Vec<Variable>,
    /// The item whose file carries the shared values: an environment's or a collection's
    /// own file, or `pustman.json` for the globals.
    item: Item,
}

impl EditTarget {
    /// A target over `item`, whose body decides the scope: an environment or a collection
    /// (their own files), or the workspace itself — the globals, whose shared values live
    /// in `pustman.json`'s `variables` field (module docs). A folder or a request holds
    /// no variables and is refused.
    pub fn new(item: Item) -> Result<Self, EditTargetError> {
        let (label, scope_id, variables) = match &item.body {
            ItemBody::Environment(environment) => (
                environment.name.clone(),
                environment.id.to_string(),
                environment.variables.clone(),
            ),
            ItemBody::Collection(collection) => (
                collection.name.clone(),
                collection.id.to_string(),
                collection.variables.clone(),
            ),
            ItemBody::Workspace(workspace) => (
                GLOBALS_LABEL.to_string(),
                GLOBALS_SCOPE_ID.to_string(),
                globals_variables(workspace)?,
            ),
            _ => return Err(EditTargetError::NotAVariableScope),
        };
        Ok(Self {
            label,
            scope_id,
            variables,
            item,
        })
    }

    /// The scope's name, in the dialog's title.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The scope id the scope's local values are keyed by (module docs).
    pub fn scope_id(&self) -> &str {
        &self.scope_id
    }

    /// The variables as the file holds them.
    pub fn variables(&self) -> &[Variable] {
        &self.variables
    }

    /// The item to hand [`workspace_store::save_item`], with `variables` in the place its
    /// body keeps them. Everything else the body holds is carried over untouched.
    fn saved_item(&self, variables: &[Variable]) -> Result<Item, EditTargetError> {
        let mut item = self.item.clone();
        match &mut item.body {
            ItemBody::Environment(environment) => environment.variables = variables.to_vec(),
            ItemBody::Collection(collection) => collection.variables = variables.to_vec(),
            ItemBody::Workspace(workspace) => {
                let value = serde_json::to_value(variables)
                    .map_err(|_| EditTargetError::VariablesUnserializable)?;
                workspace
                    .extra
                    .insert(GLOBALS_VARIABLES_FIELD.to_string(), value);
            }
            _ => return Err(EditTargetError::NotAVariableScope),
        }
        Ok(item)
    }
}

/// The globals' shared values out of `pustman.json`'s `variables` field: absent means
/// none have been written yet, present-but-unreadable is an error the caller shows.
fn globals_variables(workspace: &api_model::Workspace) -> Result<Vec<Variable>, EditTargetError> {
    match workspace.extra.get(GLOBALS_VARIABLES_FIELD) {
        None => Ok(Vec::new()),
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|_| EditTargetError::GlobalsUnreadable)
        }
    }
}

/// One variable row: its six fields, and the identity the row keeps while its key is
/// edited.
struct Row {
    /// Stable element-id base: the scope id plus the key the row opened with — never a
    /// list index (`.agents/rules/ui-render.md`).
    id: String,
    /// The key the row opened with: its identity for lookups, and the key a renamed
    /// row's local value is moved away from.
    original_key: String,
    key_input: Entity<TextInput>,
    /// The shared-value field. `None` on a secret row: it renders no shared-value field
    /// (REQ-VAR-SHARE-02).
    shared_input: Option<Entity<TextInput>>,
    /// The local-value field. `None` on a secret row: a secret's value lives in the OS
    /// keychain and never in the local store.
    local_input: Option<Entity<TextInput>>,
    kind: VariableKind,
    enabled: bool,
    description_input: Entity<TextInput>,
    /// Unknown fields the file carried, round-tripped untouched (REQ-MODEL-02).
    extra: BTreeMap<String, serde_json::Value>,
}

/// One row as the user sees it: exactly the strings [`Render`] puts in its cells, so a
/// test can assert on the display without re-deriving it. The value columns are
/// [`Option`] on purpose — `None` is a secret row rendering no field at all (the "unset"
/// of REQ-VAR-SHARE-02), not an empty cell.
pub struct DisplayedRow {
    pub key: String,
    pub shared: Option<String>,
    pub local: Option<String>,
    /// The kind word: "Default" or "Secret".
    pub kind: String,
    pub enabled: bool,
    pub description: String,
}

/// What one save writes: the file's new variables and the local store's new rows.
struct Collected {
    /// The variables to write to the file, in row order.
    variables: Vec<Variable>,
    /// The local values to write, one per row that has one.
    local_writes: Vec<LocalValue>,
    /// Keys whose local value goes away: a cleared local field — "reset to shared"
    /// (REQ-VAR-SHARE-01) — or the old key of a renamed row.
    local_clears: Vec<String>,
}

/// The variables editor: one scope's rows, their editable values and the save that lands
/// them.
pub struct EnvEditor {
    focus_handle: FocusHandle,
    /// Where a save that fails after the editor closed reports itself.
    workspace: WeakEntity<Workspace>,
    workspace_id: String,
    workspace_root: PathBuf,
    target: EditTarget,
    rows: Vec<Row>,
    store: Arc<Mutex<LocalStore>>,
    /// A save is in flight: the button says so and a second save waits for it.
    saving: bool,
}

impl EnvEditor {
    /// An editor over `inputs`' scope, its rows prefilled from the file and the local
    /// values the wire read. Nothing here touches disk.
    pub fn new(
        inputs: EnvEditorInputs,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let EnvEditorInputs {
            workspace_id,
            workspace_root,
            target,
            local_values,
            store,
        } = inputs;
        let scope_id = target.scope_id().to_owned();
        let rows = target
            .variables()
            .iter()
            .map(|variable| build_row(variable, &scope_id, &local_values, window, cx))
            .collect();
        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            workspace_id,
            workspace_root,
            target,
            rows,
            store,
            saving: false,
        }
    }

    /// Every row as the user sees it, in display order.
    pub fn displayed_rows(&self, cx: &App) -> Vec<DisplayedRow> {
        self.rows
            .iter()
            .map(|row| DisplayedRow {
                key: row.key_input.read(cx).text(),
                shared: row.shared_input.as_ref().map(|field| field.read(cx).text()),
                local: row.local_input.as_ref().map(|field| field.read(cx).text()),
                kind: kind_word(row.kind).to_string(),
                enabled: row.enabled,
                description: row.description_input.read(cx).text(),
            })
            .collect()
    }

    /// The shared-value field of the row that opened with `key` — [`None`] on a secret
    /// row, which renders no shared-value field (REQ-VAR-SHARE-02).
    pub fn shared_value_field(&self, key: &str) -> Option<Entity<TextInput>> {
        self.row_by_key(key)?.shared_input.clone()
    }

    /// The local-value field of the row that opened with `key` — [`None`] on a secret
    /// row, whose value lives in the OS keychain.
    pub fn local_value_field(&self, key: &str) -> Option<Entity<TextInput>> {
        self.row_by_key(key)?.local_input.clone()
    }

    /// Fill the shared-value field of `key`'s row as keystrokes would. `false` when the
    /// row has no such field.
    pub fn set_shared_value(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(field) = self
            .row_by_key(key)
            .and_then(|row| row.shared_input.clone())
        else {
            return false;
        };
        field.update(cx, |field, cx| field.set_text(text, cx));
        true
    }

    /// Fill the local-value field of `key`'s row as keystrokes would. `false` when the
    /// row has no such field.
    pub fn set_local_value(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.row_by_key(key).and_then(|row| row.local_input.clone()) else {
            return false;
        };
        field.update(cx, |field, cx| field.set_text(text, cx));
        true
    }

    /// Turn one row's Secret checkbox: a secret row keeps neither value field (module
    /// docs), and the text they held is destroyed with them.
    fn set_kind(
        &mut self,
        row_id: &str,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if secret {
            VariableKind::Secret
        } else {
            VariableKind::Default
        };
        let Some(row) = self.row_mut_by_id(row_id) else {
            return;
        };
        if row.kind == kind {
            return;
        }
        row.kind = kind;
        row.shared_input = (!secret).then(|| make_field(VALUE_PLACEHOLDER, "", window, cx));
        row.local_input = (!secret).then(|| make_field(VALUE_PLACEHOLDER, "", window, cx));
        cx.notify();
    }

    /// Turn one row's Enabled checkbox.
    fn set_enabled(&mut self, row_id: &str, enabled: bool, cx: &mut Context<Self>) {
        let Some(row) = self.row_mut_by_id(row_id) else {
            return;
        };
        row.enabled = enabled;
        cx.notify();
    }

    /// Save the scope as the editor shows it (module docs): the shared values to the
    /// file and the local values to storage, off the foreground thread, then close.
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let collected = match self.collect(cx) {
            Ok(collected) => collected,
            Err(message) => {
                self.report_error(message, cx);
                return;
            }
        };
        let item = match self.target.saved_item(&collected.variables) {
            Ok(item) => item,
            Err(error) => {
                self.report_error(format!("{SAVE_FAILURE_PREFIX}{error}"), cx);
                return;
            }
        };
        self.saving = true;
        cx.notify();
        let root = self.workspace_root.clone();
        let workspace_id = self.workspace_id.clone();
        let scope_id = self.target.scope_id().to_owned();
        let store = Arc::clone(&self.store);
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    persist_scope(root, item, collected, store, &workspace_id, &scope_id)
                })
                .await;
            match outcome {
                Ok(()) => {
                    if this.update(cx, |_this, cx| cx.emit(DismissEvent)).is_err() {
                        // Handled: the editor closed before the save landed. The write
                        // still happened and there is nothing left to close.
                    }
                }
                Err(error) => {
                    if this
                        .update(cx, |this, cx| {
                            this.saving = false;
                            cx.notify();
                        })
                        .is_err()
                    {
                        // Handled: the editor closed before the failed save landed; the
                        // toast below still tells the workspace.
                    }
                    if workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_toast(
                                format!("{SAVE_FAILURE_PREFIX}{error}"),
                                Severity::Error,
                                None,
                                cx,
                            );
                        })
                        .is_err()
                    {
                        // Handled: the workspace went away with its window; no one is
                        // left to tell that the save failed.
                    }
                }
            }
        })
        .detach();
    }

    /// The rows as one save writes them, or why there is not one: every key is filled and
    /// unique, a secret carries no value, and a cleared local field is a deletion.
    fn collect(&self, cx: &App) -> Result<Collected, SharedString> {
        let mut variables = Vec::with_capacity(self.rows.len());
        let mut local_writes = Vec::new();
        let mut local_clears = Vec::new();
        let mut seen = BTreeSet::new();
        for row in &self.rows {
            let key = row.key_input.read(cx).text().trim().to_owned();
            if key.is_empty() {
                return Err(SharedString::new_static(KEY_REQUIRED_MESSAGE));
            }
            if !seen.insert(key.clone()) {
                return Err(SharedString::from(format!(
                    "{DUPLICATE_KEY_PREFIX}{key}{DUPLICATE_KEY_SUFFIX}"
                )));
            }
            let secret = row.kind == VariableKind::Secret;
            let description = row.description_input.read(cx).text().trim().to_owned();
            variables.push(Variable {
                key: key.clone(),
                // A secret has no shared value, whatever its field once held
                // (REQ-VAR-SHARE-02), and an emptied field is "no value", not "".
                value: if secret {
                    None
                } else {
                    row.shared_input
                        .as_ref()
                        .map(|field| field.read(cx).text())
                        .filter(|text| !text.is_empty())
                },
                kind: row.kind,
                enabled: row.enabled,
                description: (!description.is_empty()).then_some(description),
                extra: row.extra.clone(),
            });
            if row.original_key != key {
                local_clears.push(row.original_key.clone());
            }
            if secret {
                continue;
            }
            match row.local_input.as_ref().map(|field| field.read(cx).text()) {
                Some(text) if !text.is_empty() => local_writes.push(LocalValue {
                    workspace_id: self.workspace_id.clone(),
                    scope_id: self.target.scope_id().to_owned(),
                    key: key.clone(),
                    value: text,
                }),
                _ => local_clears.push(key),
            }
        }
        Ok(Collected {
            variables,
            local_writes,
            local_clears,
        })
    }

    /// Show `message` on the workspace — where a save that cannot happen belongs
    /// (AGENTS.md: errors reach the user).
    fn report_error(&self, message: impl Into<SharedString>, cx: &mut App) {
        let message = message.into();
        if self
            .workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(message, Severity::Error, None, cx);
            })
            .is_err()
        {
            // Handled: the workspace went away with its window; there is no one left to
            // tell that the save could not happen.
        }
    }

    /// The row that opened with `key`.
    fn row_by_key(&self, key: &str) -> Option<&Row> {
        self.rows.iter().find(|row| row.original_key == key)
    }

    /// The row with this stable id.
    fn row_mut_by_id(&mut self, row_id: &str) -> Option<&mut Row> {
        self.rows.iter_mut().find(|row| row.id == row_id)
    }

    /// The table's rows and header, scrolled inside the dialog (module docs).
    fn render_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let table = self.rows.iter().fold(
            Table::new(ElementId::from("env-editor-table"))
                .column(TableColumn::new("Key"))
                .column(TableColumn::new("Shared value"))
                .column(TableColumn::new("Local value"))
                .column(TableColumn::new("Kind").width(px_to_rems(KIND_COLUMN_PX)))
                .column(TableColumn::new("Enabled").width(px_to_rems(ENABLED_COLUMN_PX)))
                .column(TableColumn::new("Description")),
            |table, row| table.row(self.render_row(row, cx)),
        );
        div()
            .id("env-editor-scroll")
            .max_h(px_to_rems(TABLE_MAX_HEIGHT_PX))
            .overflow_y_scroll()
            .child(table)
            .into_any_element()
    }

    /// One row's six cells: the three text fields a secret row keeps, the two it does
    /// not, and the two checkboxes.
    fn render_row(&self, row: &Row, cx: &mut Context<Self>) -> TableRow {
        let base = ElementId::from(row.id.clone());
        let secret = row.kind == VariableKind::Secret;
        let shared_cell = match &row.shared_input {
            Some(field) => field_cell(child_id(&base, "shared"), field, cx),
            None => absent_value_cell(child_id(&base, "shared"), SECRET_SHARED_TOOLTIP, cx),
        };
        let local_cell = match &row.local_input {
            Some(field) => field_cell(child_id(&base, "local"), field, cx),
            None => absent_value_cell(child_id(&base, "local"), SECRET_LOCAL_TOOLTIP, cx),
        };
        let kind_row_id = row.id.clone();
        let kind_weak = cx.weak_entity();
        let kind_cell = h_flex()
            .items_center()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(
                Toggle::new(child_id(&base, "kind"), "Secret", ToggleState::from(secret))
                    .label_hidden()
                    .on_change(move |state, window, cx| {
                        if kind_weak
                            .update(cx, |this, cx| {
                                this.set_kind(&kind_row_id, state.is_selected(), window, cx)
                            })
                            .is_err()
                        {
                            // Handled: the editor went away with its window; there is no row
                            // left to flip.
                        }
                    }),
            )
            .child(
                Label::new(kind_word(row.kind))
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            );
        let enabled_row_id = row.id.clone();
        let enabled_weak = cx.weak_entity();
        let enabled_cell = Toggle::new(
            child_id(&base, "enabled"),
            "Enabled",
            ToggleState::from(row.enabled),
        )
        .label_hidden()
        .on_change(move |state, _window, cx| {
            if enabled_weak
                .update(cx, |this, cx| {
                    this.set_enabled(&enabled_row_id, state.is_selected(), cx)
                })
                .is_err()
            {
                // Handled: the editor went away with its window; there is no row left to
                // flag.
            }
        });
        TableRow::new(base.clone())
            .cell(field_cell(child_id(&base, "key"), &row.key_input, cx))
            .cell(shared_cell)
            .cell(local_cell)
            .cell(kind_cell)
            .cell(enabled_cell)
            .cell(field_cell(
                child_id(&base, "description"),
                &row.description_input,
                cx,
            ))
    }
}

impl Render for EnvEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base08.rems(cx))
            .when(self.rows.is_empty(), |content| {
                content.child(Label::new(NO_VARIABLES_LABEL))
            })
            .when(!self.rows.is_empty(), |content| {
                content.child(self.render_table(cx))
            });
        let footer = h_flex()
            .w_full()
            .justify_end()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                Button::new("env-editor-cancel", "Cancel").on_click(cx.listener(
                    |_this, _event, _window, cx| {
                        cx.emit(DismissEvent);
                    },
                )),
            )
            .child(
                Button::new(
                    "env-editor-save",
                    if self.saving {
                        SharedString::new_static("Saving…")
                    } else {
                        SharedString::new_static("Save")
                    },
                )
                .loading(self.saving)
                .tooltip(Tooltip::for_action("Save the variables", &Save))
                .on_click(cx.listener(|_this, _event, window, cx| {
                    window.dispatch_action(Save.boxed_clone(), cx);
                })),
            );
        div()
            .key_context(EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save))
            .child(
                Modal::new(format!("Edit variables — {}", self.target.label()))
                    .max_width(px_to_rems(EDITOR_WIDTH_PX))
                    .content(content)
                    .footer(footer),
            )
    }
}

impl Focusable for EnvEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for EnvEditor {}

impl ModalView for EnvEditor {}

impl EnvEditor {
    /// The `env_editor::Save` handler: what its button and any key binding over
    /// [`EDITOR_CONTEXT`] dispatch.
    fn on_save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
    }
}

/// One row of the table: the variable's fields, prefilled from the file and the local
/// store, with the two value fields a secret row never gets.
fn build_row(
    variable: &Variable,
    scope_id: &str,
    local_values: &[LocalValue],
    window: &mut Window,
    cx: &mut Context<EnvEditor>,
) -> Row {
    let secret = variable.kind == VariableKind::Secret;
    let local_value = local_values
        .iter()
        .find(|value| value.scope_id == scope_id && value.key == variable.key)
        .map(|value| value.value.clone());
    Row {
        id: format!("{scope_id}:{}", variable.key),
        original_key: variable.key.clone(),
        key_input: make_field(KEY_PLACEHOLDER, &variable.key, window, cx),
        // A value a hand edit or an import left on a secret is dropped here — it reaches
        // neither a field nor the display (REQ-VAR-SHARE-02).
        shared_input: (!secret).then(|| {
            make_field(
                VALUE_PLACEHOLDER,
                variable.value.as_deref().unwrap_or_default(),
                window,
                cx,
            )
        }),
        local_input: (!secret).then(|| {
            make_field(
                VALUE_PLACEHOLDER,
                local_value.as_deref().unwrap_or_default(),
                window,
                cx,
            )
        }),
        kind: variable.kind,
        enabled: variable.enabled,
        description_input: make_field(
            DESCRIPTION_PLACEHOLDER,
            variable.description.as_deref().unwrap_or_default(),
            window,
            cx,
        ),
        extra: variable.extra.clone(),
    }
}

/// One empty-or-prefilled field, in the tab order (`response_viewer::cookie_form`'s
/// `make_field` does the same).
fn make_field(
    placeholder: &'static str,
    text: &str,
    window: &mut Window,
    cx: &mut Context<EnvEditor>,
) -> Entity<TextInput> {
    let field = cx.new(|cx| {
        TextInput::new(window, cx).with_placeholder(SharedString::new_static(placeholder))
    });
    // `text_input`'s handle defaults to not being a tab stop; every field belongs in the
    // tab order (keyboard-first).
    field.read(cx).focus_handle(cx).tab_stop(true);
    if !text.is_empty() {
        field.update(cx, |field, cx| field.set_text(text, cx));
    }
    field
}

/// A field's cell: the boxed shape `response_viewer::cookie_form` draws fields with.
fn field_cell(id: ElementId, field: &Entity<TextInput>, cx: &App) -> AnyElement {
    div()
        .id(id)
        .min_w_0()
        .flex_1()
        .px(DynamicSpacing::Base03.rems(cx))
        .border_1()
        .border_color(cx.theme().colors().border)
        .rounded_sm()
        .child(field.clone())
        .into_any_element()
}

/// A value cell with no field in it: a secret row's shared or local cell. The tooltip
/// says why there is nothing to edit here.
fn absent_value_cell(id: ElementId, tooltip: &'static str, cx: &App) -> AnyElement {
    div()
        .id(id)
        .min_w_0()
        .flex_1()
        .px(DynamicSpacing::Base03.rems(cx))
        .tooltip(Tooltip::text(tooltip).builder())
        .child(
            Label::new(NO_VALUE_LABEL)
                .size(LabelSize::Small)
                .color(Color::Muted)
                .truncate(),
        )
        .into_any_element()
}

/// A child element id under `base`: a stable, frame-unique identity for an element that
/// carries a control or a tooltip (`response_viewer`'s `child_id` shape — that one is
/// module-private to that crate).
fn child_id(base: &ElementId, name: impl Into<SharedString>) -> ElementId {
    ElementId::from((base.clone(), name))
}

/// The kind word a row's "Kind" cell shows.
fn kind_word(kind: VariableKind) -> &'static str {
    match kind {
        VariableKind::Default => "Default",
        VariableKind::Secret => "Secret",
    }
}

/// Write one scope's save: the file first (the team-visible artifact), then the local
/// store. Runs off the foreground thread — both halves are blocking IO.
fn persist_scope(
    root: PathBuf,
    item: Item,
    collected: Collected,
    store: Arc<Mutex<LocalStore>>,
    workspace_id: &str,
    scope_id: &str,
) -> anyhow::Result<()> {
    workspace_store::save_item(&root, &item)
        .map_err(|error| anyhow::anyhow!("the file could not be written: {error}"))?;
    let store_guard = lock_store(&store)?;
    for key in &collected.local_clears {
        store_guard
            .local_values()
            .delete(workspace_id, scope_id, key)
            .map_err(|error| anyhow::anyhow!("a local value could not be cleared: {error}"))?;
    }
    for local in &collected.local_writes {
        store_guard
            .local_values()
            .insert(local)
            .map_err(|error| anyhow::anyhow!("a local value could not be written: {error}"))?;
    }
    Ok(())
}

/// The store handle: a poisoned lock is an error, never a panic (the shape
/// `selector::persist_selection` uses for the same shared store).
fn lock_store(store: &Mutex<LocalStore>) -> anyhow::Result<MutexGuard<'_, LocalStore>> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))
}

/// Wire the editor to `env_editor::Open`: open it over the active workspace with the
/// [`EnvEditorInputs`] the app's wire installed.
pub fn init(cx: &mut App) {
    cx.on_action(open_env_editor);
}

/// Open the editor in the active window. The dispatch happens inside the window's own
/// update and a window cannot be updated while it is on the update stack, so the open is
/// deferred — the shape [`super::quick_look`]'s opener uses for the same situation.
fn open_env_editor(_: &Open, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace_handle) = root.downcast::<Workspace>() else {
                    return;
                };
                // Cloned out before the workspace update borrows `cx` again.
                let inputs = cx.try_global::<EnvEditorInputs>().cloned();
                let workspace = workspace_handle.downgrade();
                workspace_handle.update(cx, |workspace_handle, cx| {
                    let Some(inputs) = inputs else {
                        // Errors reach the user: an editor that cannot save would edit
                        // nothing without saying why.
                        workspace_handle.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        return;
                    };
                    workspace_handle.toggle_modal(window, cx, |window, cx| {
                        EnvEditor::new(inputs, workspace, window, cx)
                    });
                });
            })
            .is_err()
        {
            // Handled: the window closed between the keystroke and this callback; there
            // is no workspace left to open the editor over.
        }
    });
}
