//! The variables editor (PRD REQ-VAR-SHARE-01/02): one modal table over one scope's
//! variables — an environment, a collection's variables or the workspace's globals —
//! showing key, shared value, local value, kind (Default/Secret), enabled and
//! description. Saving writes the shared values to the scope's own file through
//! [`workspace_store::save_item`], the local values to `storage`'s `local_values` table
//! and the secret values to the OS keychain through [`secret_values`], all on one
//! background task; a save that fails is a toast on the workspace and leaves the editor
//! open on the rows it failed on.
//!
//! A secret row renders **no shared-value field and no local-value field**
//! (REQ-VAR-SHARE-02): a secret never has a shared value — the file records its key, kind
//! and description only, and [`workspace_store`] strips a smuggled value again on the way
//! out — and its value belongs to the OS keychain, never to the plain local store. A value
//! a hand edit or an import leaves on a secret is dropped when the rows are built: it
//! reaches neither a field nor the display. The row's one value cell is
//! [`secret_values`]'s masked field — the keychain read fills it at open and one save sets
//! what it holds back through [`secret_values`], never the file and never `storage`.
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
//! (REQ-VAR-SHARE-02). Flipping back gives empty fields — the secret row's masked field
//! excepted, which keeps its own text (the decision below).
//!
//! `Decision:` a secret row's masked field is the truth about its keychain value at save
//! time ([`secret_values`] for the keychain side). It opens on what the open-time read
//! found (masked); a non-empty field is set under the row's key; an empty field removes
//! the entry this editor *knows* about — one its open read found, or one its own row
//! renamed away from. An entry the editor could not read is never deleted blind and
//! never copied anywhere else. A kind flip keeps the field's own text: a value the user
//! typed and has not saved survives a Secret→Default→Secret round trip, and only a field
//! never touched comes back refilled from the value read at open. One save resolves final
//! ownership per keychain key — a key it sets is never also removed, so a rotation or a
//! swap of two names cannot eat the value it just wrote — and a row that stops being
//! secret takes its known keychain entry away with its local value (the flip decision
//! above).
//!
//! `Decision:` the keychain is touched on background tasks only — the open snapshot's
//! read and one save's writes ([`secret_values`]) — and a keychain failure is a visible
//! one: the open read says what it could not read inline, a save that must write a secret
//! with no keychain connected is refused before anything is written, and every failure
//! message names exactly the parts that landed before it (never one that did not). There
//! is no fallback: a secret value is never parked in the file or in `storage` because the
//! keychain said no. Save order is the staleness re-read — a refused save writes nothing
//! anywhere — then the keychain's sets (each keeping the value it replaces), the local
//! values, the file — the commit point (above) — and only after the file has landed the
//! keychain's removals. A failure before the commit point restores the values the sets
//! overwrote before it reports: what is left is at worst an orphaned entry, or — when a
//! restore itself fails — a message naming the keys that may now hold the new value.
//!
//! `Decision:` the editor reads the scope as it is *now* when it opens — the item
//! through [`workspace_store`] and the local values through `storage`, both off the
//! foreground — instead of saving from the copy [`EnvEditorInputs`] was installed with:
//! a save built from that copy reverts an earlier one and writes stale rows back over a
//! teammate's `git pull`. The fingerprint a save's staleness check compares is read from
//! the *fresh* item's own file — the path the save writes to — never from the captured
//! one, and a save whose file no longer matches it is refused with an inline message
//! that writes nothing. When no current copy can be read at all — a merge-conflicted
//! file, a deleted one, an unreadable workspace — the captured rows are never what gets
//! written: the editor opens on them with an inline explanation and saving disabled, so
//! a conflict is not overwritten and a deleted file is not resurrected. An item that has
//! never been written has no file to protect and saves normally. After a save lands, the
//! installed inputs are refreshed to what was written — path included — so they cannot
//! go stale behind an editor that saves and closes.
//!
//! `Decision:` the **file is the commit point**: a save that cannot finish leaves the
//! shared file exactly as it found it, and every failure message names exactly the parts
//! that landed before it and the parts that were undone ("Nothing was saved; the shared
//! file is unchanged — the local values could not be saved: …", "the secret values were
//! restored to the OS keychain and some local values may have changed; the shared file is
//! unchanged — it could not be written: …"). The landing order is the staleness re-read
//! — a refused save writes nothing anywhere — then the keychain's sets, the local values,
//! the file, and then the keychain's removals (the decision below). Refusals and failures
//! show inline — the editor stays open on the rows that failed (`cookie_dialog`'s choice)
//! — and only a save that fails (or warns) after the editor is already gone becomes a
//! workspace toast.
//!
//! `Decision:` Save writes the whole scope as the editor shows it — every variable to
//! the file, each row's local value to storage, a cleared local field as a deletion
//! ("reset to shared") and every secret row's key as one too (a secret keeps no local
//! value at all), with its masked field's value going to the keychain instead — and then
//! closes the editor. The save is detached rather than stored
//! in a field (closing the editor right after Save must not cancel a write that has not
//! run — the file would keep the old values while the user believes they were saved),
//! which is only sound with the task carrying its own error report: the other half of
//! this contract is the message above.
//!
//! `Decision:` the actions are declared here (`env_editor::Open`, `env_editor::Save`)
//! rather than in `pustman_actions`: this step's Edits set reaches neither that crate nor
//! `assets/keymaps/`, and the `gpui-actions-keymaps` skill's own template declares
//! actions in the feature crate. Save is dispatched by its button and by any key binding
//! over the editor's [`EDITOR_CONTEXT`]; the default binding is a wiring step's line.
//! Rows are edited in place and this step adds no add/remove-row controls.
//!
//! `Decision:` a row's element id is its position at load under the scope id, not its
//! key: an untrusted file can carry duplicate keys and `scope:key` would then collide
//! across two rows. The position is stable here — rows never move in this editor — and
//! lookups by key (`set_shared_value` and friends) address the row that opened with it.
//!
//! `Decision:` a read-only workspace (REQ-WSP-04, `workspace::read_only`) disables this
//! editor at both ends of a save: the fields take no typing (`set_row_read_only`, the
//! `request_editor` read-only wiring's counterpart here) and `save` refuses with
//! [`workspace::read_only::SAVE_REFUSED_MESSAGE`] **before** anything is written — no
//! local values, no keychain entries, no file — so the three stores cannot end up
//! disagreeing about a scope whose file never moves. The press is answered, not lost:
//! the message is inline where the rows are.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use api_model::{ItemId, Variable, VariableKind};
use gpui::{
    Action, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Global,
    Render, WeakEntity, Window,
};
use pustman_actions::environment::EditSecret;
use secrets::{SecretError, SecretKey, SecretStore, SecretValue};
use storage::{LocalStore, LocalValue};
use text_input::TextInput;
use ui::prelude::*;
use workspace::{ModalView, Workspace};
use workspace_store::{Item, ItemBody, ItemProblem, ItemProblemKind};

use crate::secret_values::{self, AppliedChange, SecretChange, SecretSnapshot, SecretState};

#[cfg(test)]
#[path = "env_editor_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "env_editor_secret_flip_tests.rs"]
mod secret_flip_tests;

#[cfg(test)]
#[path = "env_editor_save_tests.rs"]
mod save_tests;

#[cfg(test)]
#[path = "env_editor_globals_tests.rs"]
mod globals_tests;

#[cfg(test)]
#[path = "env_editor_stale_read_tests.rs"]
mod stale_read_tests;

#[cfg(test)]
#[path = "env_editor_edit_secret_tests.rs"]
mod edit_secret_tests;

#[cfg(test)]
#[path = "env_editor_read_only_tests.rs"]
mod env_editor_read_only_tests;

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

/// Why a row with neither value field has no local field: both kinds get one — the
/// ordinary row's local field, the secret row's masked keychain field.
const SECRET_LOCAL_TOOLTIP: &str =
    "A secret's value lives in the OS keychain, never in the local store.";

/// Why a secret row's value cell is a masked field, and where that value lives. A
/// tooltip on a value cell never carries the value itself (module docs).
const SECRET_KEYCHAIN_TOOLTIP: &str =
    "The value is kept in the OS keychain — never in the file or the local store.";

/// The fields' placeholders — the same "an example, not a value" shape
/// `response_viewer::cookie_form` uses.
const KEY_PLACEHOLDER: &str = "e.g. base_url";
const VALUE_PLACEHOLDER: &str = "e.g. https://api.example.test";
const DESCRIPTION_PLACEHOLDER: &str = "what this variable is for";

/// What a row with an empty key is told before a save is refused (AGENTS.md: errors are
/// actionable).
const KEY_REQUIRED_MESSAGE: &str =
    "Every variable needs a key — fill the empty Key cell before saving.";

/// What `environment::EditSecret` is told when the scope it opened has no secret row for
/// the name the action carries (S43c): the button that dispatched it names the key
/// already, so this message names neither the key nor any value (AGENTS.md › Security).
const NO_SECRET_ROW_MESSAGE: &str = "That secret is not a variable in the scope this editor has open — set it there under its own name.";

/// The prefix and suffix around the key two rows share.
const DUPLICATE_KEY_PREFIX: &str = "Two variables share the key `";
const DUPLICATE_KEY_SUFFIX: &str = "` — keys must be unique before saving.";

/// The one message the opener shows when the app has not installed [`EnvEditorInputs`]
/// (AGENTS.md: errors reach the user, never only a log).
const NOT_WIRED_MESSAGE: &str = "The variables editor cannot save changes — the workspace folder and local store are not connected";

/// What every failed save is shown with, so the message says what did not happen before
/// it quotes the reason.
const SAVE_FAILURE_PREFIX: &str = "Could not save the variables — ";

/// What a save whose file changed on disk since the editor read it is refused with: the
/// staleness re-read runs first (module docs), so a refused save writes nothing anywhere
/// and the editor keeps its rows open on the teammate's file.
const CHANGED_ON_DISK_MESSAGE: &str = "The variables changed on disk since this editor was opened — close it and reopen it to see the current values. Nothing was saved.";

/// What a failure during the local values or the shared-file write is shown with, after
/// [`landed_note`] says which parts landed and which were undone (module docs).
const LOCALS_FAILED_MESSAGE: &str =
    "the shared file is unchanged — the local values could not be saved: ";
const FILE_FAILED_MESSAGE: &str = "the shared file is unchanged — it could not be written: ";

/// What the staleness check's own read failure is shown with: the re-read runs first
/// (module docs), so nothing has been written at that point.
const UNREADABLE_MESSAGE: &str =
    "Nothing was saved — the shared file could not be read to check it for changes: ";

/// The prefix every "this scope could not be re-read" message opens with (module docs),
/// before the cause.
const UNREADABLE_SCOPE_PREFIX: &str = "These variables could not be re-read from disk, so the rows show the last copy this editor was given: ";

/// What the editor opens with when the local values could not be read at open (module
/// docs): the column shows the wire's last-known values and says so.
const LOCALS_UNVERIFIED_PREFIX: &str =
    "The local values could not be read — the Local value column may be out of date: ";

/// What every "the secret values could not be read at open" message opens with (module
/// docs), before the per-key causes.
const SECRET_READ_PREFIX: &str = "The saved secret values could not be read from the OS keychain — those fields start empty and an empty field saves nothing: ";

/// What a keychain write that failed is shown with, after [`landed_note`]: no value ever
/// appears in it, only the keys a restore could not put back (module docs).
const SECRET_SET_FAILED_MESSAGE: &str = "the secret values could not be saved to the OS keychain: ";
const SECRET_SET_FAILED_REST_MESSAGE: &str = "the rest could not be saved to the OS keychain: ";

/// What [`landed_note`] says about the keychain after a failure rolled the sets back
/// (module docs): every overwritten value restored, or the keys — never the values —
/// whose restore failed.
const KEYCHAIN_RESTORED_NOTE: &str = "the secret values were restored to the OS keychain";
const KEYCHAIN_MAY_HOLD_NOTE: &str = "the OS keychain may now hold the new value for ";

/// What [`landed_note`] says about the local values: some of the clears/writes landed
/// before the failure, or the whole part landed.
const LOCALS_PARTIAL_NOTE: &str = "some local values may have changed";
const LOCALS_SAVED_NOTE: &str = "the local values were saved";

/// What a removal that could not run after the save landed is reported with: the save
/// itself is complete and only an orphaned entry is left behind (module docs). Shown as
/// a workspace toast after the editor closes.
const REMOVE_FAILED_MESSAGE: &str = "The variables were saved, but an old keychain entry could not be removed and is left in the OS keychain: ";

/// What a save that must write a secret with no keychain connected is refused with,
/// before anything at all is written (module docs).
const NO_KEYCHAIN_MESSAGE: &str = "Nothing was saved — a secret value can only be kept in the OS keychain, and it is not connected";

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

    /// The item's stable id: what [`workspace_store::open`] keys the current copy by.
    fn item_id(&self) -> Option<ItemId> {
        self.item.id()
    }

    /// The file the shared values live in, relative to the workspace root, once the item
    /// has been written: what the save's staleness check reads (module docs).
    fn file_path(&self) -> Option<&Path> {
        self.item.path.as_deref()
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
    /// Stable element-id base: the scope id plus the row's position at load. Not the
    /// key: an untrusted file can carry duplicate keys, which `scope:key` would collide
    /// on (module docs). Rows never move here, so the position is stable.
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
    /// The secret row's masked value field ([`secret_values`]). `None` on a non-secret
    /// row: only a secret has a keychain value to type into.
    secret_input: Option<Entity<TextInput>>,
    /// The value the open read found — what the masked field opens on and what a flip
    /// back refills it with when the field was never touched (module docs). Held as
    /// [`SecretValue`] so nothing that formats a row can print it.
    secret_opened_value: Option<SecretValue>,
    /// The masked field's own text, kept while a kind flip has no field to hold it
    /// (module docs): what the flip back restores, so a typed value survives the round
    /// trip. Also a [`SecretValue`] — it is user-typed secret text.
    secret_draft: Option<SecretValue>,
    /// The key a keychain entry is known to live under — the key the open read found it
    /// under. What one save removes or replaces (module docs); `None` when no entry was
    /// found, so an entry this editor could not read is never deleted blind.
    secret_entry_key: Option<String>,
    kind: VariableKind,
    enabled: bool,
    description_input: Entity<TextInput>,
    /// Unknown fields the file carried, round-tripped untouched (REQ-MODEL-02).
    extra: BTreeMap<String, serde_json::Value>,
}

/// One row as the user sees it: exactly the strings [`Render`] puts in its cells, so a
/// test can assert on the display without re-deriving it. The value columns are
/// [`Option`] on purpose — `None` is a secret row rendering no field at all (the "unset"
/// of REQ-VAR-SHARE-02), not an empty cell. [`DisplayedRow::secret`] is the mirror image:
/// `Some` on a secret row only, and always the masked stand-ins — it can never carry the
/// value (module docs).
pub struct DisplayedRow {
    pub key: String,
    pub shared: Option<String>,
    pub local: Option<String>,
    /// The secret row's masked field as its cell paints it: one stand-in per character of
    /// the value, never the value itself (`TextInput::display_text`).
    pub secret: Option<String>,
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
    /// (REQ-VAR-SHARE-01) — a secret row's key (a secret keeps no local value at all),
    /// or the old key of a renamed row.
    local_clears: Vec<String>,
    /// The keychain sets one save performs ([`secret_values`]), in row order: each
    /// secret's field value under its key. Applied before anything else (module docs).
    secret_sets: Vec<SecretChange>,
    /// The keychain removals one save performs, in row order — every key this save sets
    /// is filtered out of here first (module docs), and what is left runs only after the
    /// shared file has landed.
    secret_removes: Vec<SecretChange>,
}

/// What [`read_scope`] hands the editor when it opens (module docs): the scope as it is
/// on disk and in the local store right now, plus the file fingerprint a save
/// re-checks.
pub struct OpenedScope {
    target: EditTarget,
    local_values: Vec<LocalValue>,
    /// The scope's file as it was read — [`None`] when the item has never been written,
    /// so it has no file to go stale.
    fingerprint: Option<Vec<u8>>,
    /// What the editor opens saying, when the scope could not be verified.
    notice: Option<SharedString>,
    /// No current copy of the scope could be read (module docs): the rows are the
    /// wire's last copy and nothing may be saved over the file they describe.
    save_blocked: bool,
    /// The secret rows' keychain state as the open-time read found it (module docs).
    secrets: Vec<SecretSnapshot>,
}

/// What one save writes and re-checks first, gathered on the foreground before the task
/// starts (module docs).
struct SavePlan {
    root: PathBuf,
    /// The file the shared values live in, when the scope has one.
    file_path: Option<PathBuf>,
    /// The file as the editor read it: what the staleness check compares against.
    fingerprint: Option<Vec<u8>>,
    item: Item,
    collected: Collected,
    workspace_id: String,
    scope_id: String,
    /// The keychain the secret changes go to. [`None`] means none connected, which a save
    /// that has secret changes refuses before it writes (module docs).
    secret_store: Option<Arc<dyn SecretStore>>,
}

/// What one save wrote: the file it landed, and a warning that must still reach the
/// user — a keychain entry the removals could not clean up. The save itself landed
/// (module docs), so this is reported after the editor closes, as a workspace toast.
struct Written {
    file: PathBuf,
    warning: Option<String>,
}

/// Why a save did not write everything: the full message the user is shown, said against
/// what actually landed before it (module docs — [`landed_note`]).
struct SaveFailure(String);

impl SaveFailure {
    /// What the user is shown, inline where the rows are (module docs).
    fn message(self) -> String {
        self.0
    }
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
    /// The scope's file as it was read: what a save re-checks before it writes (module
    /// docs).
    fingerprint: Option<Vec<u8>>,
    /// No current copy of the scope could be read (module docs): the rows shown are the
    /// wire's last copy and saving is refused.
    save_blocked: bool,
    /// The keychain the secret rows' values live in ([`secret_values`]), when one is
    /// connected.
    secret_store: Option<Arc<dyn SecretStore>>,
    /// A save is in flight: the button says so and a second save waits for it.
    saving: bool,
    /// Why the last save did not happen (or why the local values may be out of date),
    /// shown in the dialog — inline, where the rows are (the `cookie_dialog` shape).
    error: Option<SharedString>,
}

impl EnvEditor {
    /// An editor over the scope `opened` read (module docs): its rows are what the file
    /// and the local store hold right now. Nothing here touches disk.
    pub fn new(
        inputs: EnvEditorInputs,
        opened: OpenedScope,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let EnvEditorInputs {
            workspace_id,
            workspace_root,
            store,
            ..
        } = inputs;
        let OpenedScope {
            target,
            local_values,
            fingerprint,
            notice,
            save_blocked,
            secrets,
        } = opened;
        let scope_id = target.scope_id().to_owned();
        // Annotated: with the type left to the struct field below, `for row in &rows`
        // overflows trait resolution on macOS (E0275 through itertools' IntoIterator impls).
        let rows: Vec<Row> = target
            .variables()
            .iter()
            .enumerate()
            .map(|(index, variable)| {
                build_row(
                    variable,
                    index,
                    &scope_id,
                    &local_values,
                    &secrets,
                    window,
                    cx,
                )
            })
            .collect();
        // A read-only workspace takes no typing anywhere in the dialog (REQ-WSP-04).
        if workspace::read_only::is_read_only(cx) {
            for row in &rows {
                set_row_read_only(row, true, cx);
            }
        }
        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            workspace_id,
            workspace_root,
            target,
            rows,
            store,
            secret_store: secret_values::store(cx),
            fingerprint,
            save_blocked,
            saving: false,
            error: notice,
        }
    }

    /// Why the last save did not happen, while the dialog says so.
    pub fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    /// Every row as the user sees it, in display order.
    pub fn displayed_rows(&self, cx: &App) -> Vec<DisplayedRow> {
        self.rows
            .iter()
            .map(|row| DisplayedRow {
                key: row.key_input.read(cx).text(),
                shared: row.shared_input.as_ref().map(|field| field.read(cx).text()),
                local: row.local_input.as_ref().map(|field| field.read(cx).text()),
                secret: row
                    .secret_input
                    .as_ref()
                    .map(|field| field.read(cx).display_text()),
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

    /// The masked value field of the row that opened with `key` ([`secret_values`]) —
    /// [`Some`] on a secret row only.
    pub fn secret_value_field(&self, key: &str) -> Option<Entity<TextInput>> {
        self.row_by_key(key)?.secret_input.clone()
    }

    /// Fill the masked value field of `key`'s row as keystrokes would. `false` when the
    /// row has no such field.
    pub fn set_secret_value(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(field) = self
            .row_by_key(key)
            .and_then(|row| row.secret_input.clone())
        else {
            return false;
        };
        field.update(cx, |field, cx| field.set_text(text, cx));
        true
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

    /// Fill the key field of `key`'s row as keystrokes would — a rename. `false` when
    /// there is no such row.
    pub fn set_key(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.row_by_key(key).map(|row| row.key_input.clone()) else {
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

    /// Flip one row's kind as its Secret checkbox does: a secret row keeps neither value
    /// field (module docs), and the text they held is destroyed with them. `false` when
    /// there is no such row.
    pub fn set_secret(
        &mut self,
        key: &str,
        secret: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(row) = self.row_by_key(key) else {
            return false;
        };
        let row_id = row.id.clone();
        self.set_kind(&row_id, secret, window, cx);
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
        if secret {
            // Flipping back keeps the field's own text (module docs): a typed, unsaved
            // value survives the round trip. Only a field never touched comes back
            // refilled from the value read at open.
            let restore = row
                .secret_draft
                .as_ref()
                .or(row.secret_opened_value.as_ref())
                .map(|value| value.expose().to_string())
                .unwrap_or_default();
            row.secret_input = Some(secret_values::make_secret_field(&restore, window, cx));
        } else {
            // Flipping away keeps the field's text in the row — the draft the flip back
            // restores (module docs) — instead of destroying it with the field.
            let draft = row
                .secret_input
                .as_ref()
                .map(|field| SecretValue::new(field.read(cx).text()));
            row.secret_input = None;
            row.secret_draft = draft;
        }
        // The fields a flip just built follow the workspace too (REQ-WSP-04).
        if workspace::read_only::is_read_only(cx) {
            set_row_read_only(row, true, cx);
        }
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

    /// Save the scope as the editor shows it (module docs): the staleness re-read — a
    /// refused save writes nothing anywhere — then the keychain's sets, the local values,
    /// the shared file (the commit point) and the keychain's removals, all off the
    /// foreground thread, then close.
    fn save(&mut self, cx: &mut Context<Self>) {
        // A read-only workspace refuses before anything is written anywhere (REQ-WSP-04):
        // no local values, no keychain entries, no file (module docs). The press is
        // answered with the message every other read-only save press gets.
        if workspace::read_only::is_read_only(cx) {
            self.show_error(
                SharedString::new_static(workspace::read_only::SAVE_REFUSED_MESSAGE),
                cx,
            );
            return;
        }
        // A scope with no current copy on disk is never saved from the wire's last one
        // (module docs): the banner already says why.
        if self.saving || self.save_blocked {
            return;
        }
        let collected = match self.collect(cx) {
            Ok(collected) => collected,
            Err(message) => {
                self.show_error(message, cx);
                return;
            }
        };
        let item = match self.target.saved_item(&collected.variables) {
            Ok(item) => item,
            Err(error) => {
                self.show_error(format!("{SAVE_FAILURE_PREFIX}{error}"), cx);
                return;
            }
        };
        // A secret value has exactly one home (module docs): with no keychain connected
        // the save is refused before anything is written — never dropped, never written
        // somewhere else.
        if (!collected.secret_sets.is_empty() || !collected.secret_removes.is_empty())
            && self.secret_store.is_none()
        {
            self.show_error(SharedString::new_static(NO_KEYCHAIN_MESSAGE), cx);
            return;
        }
        self.error = None;
        self.saving = true;
        cx.notify();
        let plan = SavePlan {
            root: self.workspace_root.clone(),
            file_path: self.target.file_path().map(Path::to_path_buf),
            fingerprint: self.fingerprint.clone(),
            item,
            collected,
            workspace_id: self.workspace_id.clone(),
            scope_id: self.target.scope_id().to_owned(),
            secret_store: self.secret_store.clone(),
        };
        // What the installed inputs are refreshed to once this lands (module docs).
        let saved_item = plan.item.clone();
        let saved_locals = plan.collected.local_writes.clone();
        let store = Arc::clone(&self.store);
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { persist_scope(plan, store) })
                .await;
            match outcome {
                Ok(written) => {
                    if this
                        .update(cx, |this, cx| {
                            this.refresh_installed_inputs(
                                &saved_item,
                                &written.file,
                                &saved_locals,
                                cx,
                            );
                            cx.emit(DismissEvent);
                        })
                        .is_err()
                    {
                        // Handled: the editor closed before the save landed. The write
                        // still happened and there is nothing left to close.
                    }
                    // The save landed, so its one warning — a removal that could not
                    // run (module docs) — is reported where a dismissed editor's
                    // messages go.
                    if let Some(warning) = written.warning
                        && workspace
                            .update(cx, |workspace, cx| {
                                workspace.show_toast(warning, Severity::Warning, None, cx);
                            })
                            .is_err()
                    {
                        // Handled: the workspace went away with its window; the saved
                        // variables are on disk either way.
                    }
                }
                Err(failure) => {
                    let message = failure.message();
                    let delivered = this.update(cx, |this, cx| {
                        this.saving = false;
                        this.show_error(message.clone(), cx);
                    });
                    if delivered.is_err()
                        && workspace
                            .update(cx, |workspace, cx| {
                                workspace.show_toast(message, Severity::Error, None, cx);
                            })
                            .is_err()
                    {
                        // Handled: the editor and the workspace both went away with
                        // the window; there is no one left to tell that the save failed.
                    }
                }
            }
        })
        .detach();
    }

    /// Point the installed [`EnvEditorInputs`] at what was just saved (module docs), so
    /// the wire's copy cannot go stale behind an editor that saves and closes.
    fn refresh_installed_inputs(
        &self,
        saved_item: &Item,
        written: &Path,
        saved_locals: &[LocalValue],
        cx: &mut Context<Self>,
    ) {
        if !cx.has_global::<EnvEditorInputs>() {
            return;
        }
        let inputs = cx.global_mut::<EnvEditorInputs>();
        if inputs.workspace_id != self.workspace_id
            || inputs.target.scope_id() != self.target.scope_id()
        {
            return;
        }
        let mut saved_item = saved_item.clone();
        // The file `save_item` wrote is where the item lives now: a never-written item's
        // first save gives it a path for the first time, and an item without one is
        // re-suffixed instead of written over its own file next time.
        saved_item.path = Some(written.to_path_buf());
        // The body just came out of `saved_item`, which built it, so this re-read of the
        // variables cannot fail where that build did not.
        if let Ok(target) = EditTarget::new(saved_item) {
            inputs.target = target;
        }
        inputs.local_values = saved_locals.to_vec();
    }

    /// The rows as one save writes them, or why there is not one: every key is filled and
    /// unique, a secret carries no value, and a cleared local field is a deletion.
    fn collect(&self, cx: &App) -> Result<Collected, SharedString> {
        let mut variables = Vec::with_capacity(self.rows.len());
        let mut local_writes = Vec::new();
        let mut local_clears = Vec::new();
        let mut secret_sets = Vec::new();
        let mut secret_removes = Vec::new();
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
                // A secret keeps no local value at all (its value is the OS keychain's):
                // whatever a previous session or this row's own kind flip left under
                // this key goes, or the plaintext outlives the flip (REQ-VAR-SHARE-02).
                local_clears.push(key.clone());
                // The keychain side (module docs): the masked field's text is the truth.
                let text = row
                    .secret_input
                    .as_ref()
                    .map(|field| field.read(cx).text())
                    .unwrap_or_default();
                if text.is_empty() {
                    // An empty field removes the entry this editor knows about (module
                    // docs) and nothing else.
                    if let Some(entry_key) = &row.secret_entry_key {
                        secret_removes.push(
                            SecretChange::remove(
                                &self.workspace_id,
                                self.target.scope_id(),
                                entry_key,
                            )
                            .map_err(secret_write_failure)?,
                        );
                    }
                } else {
                    secret_sets.push(
                        SecretChange::set(
                            &self.workspace_id,
                            self.target.scope_id(),
                            &key,
                            // Kept exact and untrimmed, like any password
                            // (`secret_values`'s contract).
                            &text,
                        )
                        .map_err(secret_write_failure)?,
                    );
                    if let Some(entry_key) = &row.secret_entry_key
                        && entry_key != &key
                    {
                        secret_removes.push(
                            SecretChange::remove(
                                &self.workspace_id,
                                self.target.scope_id(),
                                entry_key,
                            )
                            .map_err(secret_write_failure)?,
                        );
                    }
                }
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
            // A row that is no longer a secret takes its known keychain entry away with
            // it (module docs): the value must not outlive the kind flip.
            if let Some(entry_key) = &row.secret_entry_key {
                secret_removes.push(
                    SecretChange::remove(&self.workspace_id, self.target.scope_id(), entry_key)
                        .map_err(secret_write_failure)?,
                );
            }
        }
        // Final keychain ownership (module docs): a key this save sets is never also
        // removed — a rotation's or a swap's old name can name a key another row is
        // setting right now, and the removal would eat the value just written.
        let set_keys: BTreeSet<String> = secret_sets
            .iter()
            .map(|change| change.key().account())
            .collect();
        secret_removes.retain(|change| !set_keys.contains(&change.key().account()));
        Ok(Collected {
            variables,
            local_writes,
            local_clears,
            secret_sets,
            secret_removes,
        })
    }

    /// Show `message` in the dialog — inline, where the rows it failed on are
    /// (AGENTS.md: errors reach the user).
    fn show_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
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
            Some(field) => field_cell(child_id(&base, "shared"), field, None, cx),
            None => absent_value_cell(child_id(&base, "shared"), SECRET_SHARED_TOOLTIP, cx),
        };
        let local_cell = match &row.local_input {
            Some(field) => field_cell(child_id(&base, "local"), field, None, cx),
            None => match &row.secret_input {
                // A secret row's value cell is the masked keychain field (module docs).
                Some(field) => field_cell(
                    child_id(&base, "secret"),
                    field,
                    Some(SECRET_KEYCHAIN_TOOLTIP),
                    cx,
                ),
                None => absent_value_cell(child_id(&base, "local"), SECRET_LOCAL_TOOLTIP, cx),
            },
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
            .cell(field_cell(child_id(&base, "key"), &row.key_input, None, cx))
            .cell(shared_cell)
            .cell(local_cell)
            .cell(kind_cell)
            .cell(enabled_cell)
            .cell(field_cell(
                child_id(&base, "description"),
                &row.description_input,
                None,
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
            .when_some(self.error.clone(), |content, error| {
                content.child(Banner::new("env-editor-error", error).severity(Severity::Error))
            })
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
                .disabled(self.save_blocked)
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
/// store, with the two value fields a secret row never gets and the masked keychain field
/// it gets instead. `index` is the row's position at load — its stable identity (module
/// docs); `secrets` is the open-time keychain read (module docs).
fn build_row(
    variable: &Variable,
    index: usize,
    scope_id: &str,
    local_values: &[LocalValue],
    secrets: &[SecretSnapshot],
    window: &mut Window,
    cx: &mut Context<EnvEditor>,
) -> Row {
    let secret = variable.kind == VariableKind::Secret;
    let local_value = local_values
        .iter()
        .find(|value| value.scope_id == scope_id && value.key == variable.key)
        .map(|value| value.value.clone());
    // What the keychain read found — never the value a hand edit or an import may have
    // smuggled into the file, which is dropped here (REQ-VAR-SHARE-02). The field opens
    // on the stored value only when there is one, and only then does clearing it mean
    // "remove it" (module docs).
    let (secret_opened_value, secret_entry_key) = match secrets
        .iter()
        .find(|snapshot| snapshot.key == variable.key)
        .map(|snapshot| &snapshot.state)
    {
        Some(SecretState::Found(value)) => (Some(value.clone()), Some(variable.key.clone())),
        _ => (None, None),
    };
    let id = format!("{scope_id}:row:{index}");
    let secret_input = secret.then(|| {
        secret_values::make_secret_field(
            secret_opened_value
                .as_ref()
                .map(|value| value.expose())
                .unwrap_or_default(),
            window,
            cx,
        )
    });
    Row {
        id,
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
        secret_input,
        secret_opened_value,
        secret_draft: None,
        secret_entry_key,
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

/// Set read-only on every field one row holds — the key and description fields and
/// whichever value fields its kind keeps (REQ-WSP-04): a read-only workspace takes no
/// typing, so nothing accumulates behind the save it refuses anyway. The fields are
/// built (and re-built on a kind flip) wherever rows are, so the push runs there too.
fn set_row_read_only(row: &Row, read_only: bool, cx: &mut App) {
    let mut fields = vec![row.key_input.clone(), row.description_input.clone()];
    fields.extend(row.shared_input.clone());
    fields.extend(row.local_input.clone());
    fields.extend(row.secret_input.clone());
    for field in fields {
        field.update(cx, |field, cx| field.set_read_only(read_only, cx));
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

/// A field's cell: the boxed shape `response_viewer::cookie_form` draws fields with. The
/// tooltip is a static explanation of the cell, never the field's text — a secret cell's
/// tooltip must outlive its value without leaking it (module docs).
fn field_cell(
    id: ElementId,
    field: &Entity<TextInput>,
    tooltip: Option<&'static str>,
    cx: &App,
) -> AnyElement {
    let cell = div()
        .id(id)
        .min_w_0()
        .flex_1()
        .px(DynamicSpacing::Base03.rems(cx))
        .border_1()
        .border_color(cx.theme().colors().border)
        .rounded_sm();
    let cell = match tooltip {
        Some(text) => cell.tooltip(Tooltip::text(text).builder()),
        None => cell,
    };
    cell.child(field.clone()).into_any_element()
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

/// The scope as it is right now (module docs): the item through `workspace_store`, the
/// local values through the store, and the file fingerprint — all read off the
/// foreground. `captured` is what the wire installed: the rows the editor opens on when
/// no current copy can be read, with saving blocked unless the item has never been
/// written.
fn read_scope(
    workspace_root: &Path,
    captured: &EditTarget,
    workspace_id: &str,
    captured_locals: Vec<LocalValue>,
    store: &Mutex<LocalStore>,
    secret_store: Option<&Arc<dyn SecretStore>>,
) -> OpenedScope {
    let (local_values, locals_notice) = match read_local_values(store, workspace_id) {
        Ok(local_values) => (local_values, None),
        Err(error) => (
            captured_locals,
            Some(SharedString::from(format!(
                "{LOCALS_UNVERIFIED_PREFIX}{error}"
            ))),
        ),
    };
    let mut opened = match read_current(workspace_root, captured) {
        Ok((target, fingerprint)) => OpenedScope {
            target,
            local_values,
            fingerprint,
            notice: locals_notice,
            save_blocked: false,
            secrets: Vec::new(),
        },
        Err(reason) => {
            // The captured rows are shown as a reference only (module docs): for an item
            // that has (or had) a file there is nothing safe to write over it, and for a
            // never-written one the reason is worth showing even though saving is fine.
            let save_blocked = captured.file_path().is_some();
            let notice = if save_blocked {
                SharedString::from(format!(
                    "{reason} Saving is disabled so nothing is overwritten or brought back."
                ))
            } else {
                reason
            };
            OpenedScope {
                target: captured.clone(),
                local_values,
                fingerprint: None,
                notice: Some(notice),
                save_blocked,
                secrets: Vec::new(),
            }
        }
    };
    // The secret rows' keychain state (module docs): read for whichever scope the editor
    // opens on — the fresh copy or, when there is none, the captured one.
    let (secrets, secret_notice) = read_secrets(
        secret_store,
        workspace_id,
        opened.target.scope_id(),
        opened.target.variables(),
    );
    opened.secrets = secrets;
    opened.notice = match (opened.notice.take(), secret_notice) {
        (Some(notice), Some(secret_notice)) => {
            Some(SharedString::from(format!("{notice} {secret_notice}")))
        }
        (notice, secret_notice) => notice.or(secret_notice),
    };
    opened
}

/// The secret rows' keychain state as the open-time read found it (module docs), and why
/// any of it could not be read — one inline message naming every key that failed. Runs
/// where the caller runs: off the foreground.
fn read_secrets(
    secret_store: Option<&Arc<dyn SecretStore>>,
    workspace_id: &str,
    scope_id: &str,
    variables: &[Variable],
) -> (Vec<SecretSnapshot>, Option<SharedString>) {
    let mut snapshots = Vec::new();
    let mut failures = Vec::new();
    for variable in variables
        .iter()
        .filter(|variable| variable.kind == VariableKind::Secret)
    {
        let snapshot =
            secret_values::read_secret(secret_store, workspace_id, scope_id, &variable.key);
        if let SecretState::Unreadable(reason) = &snapshot.state {
            failures.push(format!("`{}`: {reason}", variable.key));
        }
        snapshots.push(snapshot);
    }
    if failures.is_empty() {
        (snapshots, None)
    } else {
        (
            snapshots,
            Some(SharedString::from(format!(
                "{SECRET_READ_PREFIX}{}",
                failures.join("; ")
            ))),
        )
    }
}

/// A key that cannot be named in the OS keychain is a refused save, said the way every
/// other refusal is (the reason names the key, never a value).
fn secret_write_failure(error: SecretError) -> SharedString {
    SharedString::from(format!("{SAVE_FAILURE_PREFIX}{error}"))
}

/// The scope's current copy and the file bytes a save re-checks (module docs), or why
/// there is no current copy — which the caller answers with a refused save rather than
/// the captured rows.
fn read_current(
    workspace_root: &Path,
    captured: &EditTarget,
) -> Result<(EditTarget, Option<Vec<u8>>), SharedString> {
    // The captured path is read first when the current copy is expected at the same
    // place: the earlier read is the safer fingerprint, because a write racing the
    // workspace scan then fails the save's check instead of passing it.
    let captured_bytes = match captured.file_path() {
        Some(relative) => read_file_state(&workspace_root.join(relative))
            .map_err(|error| unreadable(&format!("the file could not be read — {error}")))?,
        // An item that has never been written has no file to read.
        None => None,
    };
    let snapshot = workspace_store::open(workspace_root)
        .map_err(|error| unreadable(&format!("the workspace could not be read — {error}")))?;
    let problem = snapshot.problems.iter().find(|problem| {
        captured
            .file_path()
            .is_some_and(|path| problem.path == path)
    });
    let Some(item) = captured
        .item_id()
        .and_then(|id| snapshot.items.get(&id))
        .cloned()
    else {
        if captured.file_path().is_none() {
            // Never written: there is no file to be stale about, so the captured rows
            // are the truth and a save creates the file.
            return Ok((captured.clone(), None));
        }
        return Err(refusal_reason(problem));
    };
    let target = EditTarget::new(item).map_err(|error| unreadable(&error.to_string()))?;
    let fresh_path = match target.file_path() {
        Some(fresh_path) => fresh_path.to_path_buf(),
        // `workspace_store::open` gives every item a file; without one there is nothing
        // to check and the save writes where the item says.
        None => return Ok((target, None)),
    };
    let fingerprint = if captured.file_path() == Some(fresh_path.as_path()) {
        captured_bytes
    } else {
        // The file moved since the wire captured it (or was never captured): the
        // fingerprint comes from the fresh item's own path (module docs).
        read_file_state(&workspace_root.join(&fresh_path))
            .map_err(|error| unreadable(&format!("the file could not be read — {error}")))?
    };
    let Some(fingerprint) = fingerprint else {
        // The file vanished between the scan and this read: there is no current copy to
        // save over either.
        return Err(refusal_reason(None));
    };
    Ok((target, Some(fingerprint)))
}

/// The message a scope that cannot be re-read opens with (module docs).
fn unreadable(reason: &str) -> SharedString {
    SharedString::from(format!("{UNREADABLE_SCOPE_PREFIX}{reason}"))
}

/// Why there is no current copy of the scope: the problem [`workspace_store`] reported
/// for its file, or the plain "it is gone" when it reported none.
fn refusal_reason(problem: Option<&ItemProblem>) -> SharedString {
    let detail = match problem.map(|problem| &problem.kind) {
        Some(ItemProblemKind::ConflictMarkers) => "the file has merge-conflict markers",
        Some(ItemProblemKind::InvalidJson { .. }) => "the file is not valid JSON",
        Some(ItemProblemKind::DuplicateId { .. }) => "several files carry this item's id",
        None => "the file is gone from disk or no longer holds these variables",
    };
    unreadable(detail)
}

/// The file's current bytes, or [`None`] when it does not exist: the fingerprint a save
/// compares against (module docs).
fn read_file_state(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow::anyhow!("the file could not be read: {error}")),
    }
}

/// The scope's local values as the store holds them right now.
fn read_local_values(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
) -> anyhow::Result<Vec<LocalValue>> {
    let store_guard = lock_store(store)?;
    Ok(store_guard.local_values().list(workspace_id)?)
}

/// Write one save off the foreground (module docs): the keychain's sets first, then the
/// local values, then the staleness check and the shared file — the file is the commit
/// point — and only after it has landed the keychain's removals. Returns the file it
/// wrote and any removal that could not run (at worst an orphaned entry, never a lost
/// one).
fn persist_scope(plan: SavePlan, store: Arc<Mutex<LocalStore>>) -> Result<Written, SaveFailure> {
    // 0. The staleness re-read (module docs): a refused save writes nothing anywhere —
    // no keychain entry, no local value, no file.
    if let Some(relative) = &plan.file_path {
        let current = read_file_state(&plan.root.join(relative))
            .map_err(|error| SaveFailure(format!("{UNREADABLE_MESSAGE}{error}")))?;
        if current != plan.fingerprint {
            return Err(SaveFailure(CHANGED_ON_DISK_MESSAGE.to_string()));
        }
    }
    // 1. The keychain's sets (module docs), each keeping the value it replaces so a later
    // failure can put it back.
    let mut applied: Vec<AppliedChange> = Vec::new();
    for change in &plan.collected.secret_sets {
        match apply_backed_up(change, &plan) {
            Ok(applied_change) => applied.push(applied_change),
            Err(error) => {
                let restores_failed = rollback_applied(&plan, &applied);
                let note = landed_note(
                    (!applied.is_empty()).then(|| keychain_note(&restores_failed)),
                    None,
                );
                let tail = if applied.is_empty() {
                    SECRET_SET_FAILED_MESSAGE
                } else {
                    SECRET_SET_FAILED_REST_MESSAGE
                };
                return Err(SaveFailure(format!("{note}; {tail}{error}")));
            }
        }
    }
    // 2. The local values.
    let mut locals_touched = 0usize;
    if let Err(error) = write_locals(&plan, &store, &mut locals_touched) {
        let restores_failed = rollback_applied(&plan, &applied);
        let note = landed_note(
            (!applied.is_empty()).then(|| keychain_note(&restores_failed)),
            (locals_touched > 0).then(|| LOCALS_PARTIAL_NOTE.to_string()),
        );
        return Err(SaveFailure(format!(
            "{note}; {LOCALS_FAILED_MESSAGE}{error}"
        )));
    }
    // 3. The shared file (module docs): the commit point.
    let file = match workspace_store::save_item(&plan.root, &plan.item) {
        Ok(file) => file,
        Err(error) => {
            let restores_failed = rollback_applied(&plan, &applied);
            let note = landed_note(
                (!applied.is_empty()).then(|| keychain_note(&restores_failed)),
                (locals_touched > 0).then(|| LOCALS_SAVED_NOTE.to_string()),
            );
            return Err(SaveFailure(format!("{note}; {FILE_FAILED_MESSAGE}{error}")));
        }
    };
    // 4. The keychain's removals (module docs): only now, after the commit point. A
    // removal that fails leaves at worst an orphaned entry and is reported as a warning
    // rather than failing a save that has already landed.
    let mut warnings = Vec::new();
    for change in &plan.collected.secret_removes {
        if let Err(error) = apply_change(change, &plan) {
            warnings.push(format!("{REMOVE_FAILED_MESSAGE}{error}"));
        }
    }
    Ok(Written {
        file,
        warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
    })
}

/// One keychain change against the store this save was given (module docs): a save with
/// keychain changes is refused before it starts when none is connected, and reaching this
/// is a bug that must fail loudly rather than drop the value.
fn apply_change(change: &SecretChange, plan: &SavePlan) -> Result<(), SecretError> {
    match plan.secret_store.as_ref() {
        Some(secret_store) => change.apply(secret_store),
        None => Err(SecretError::BackendUnavailable {
            reason: secret_values::NOT_CONNECTED.to_string(),
        }),
    }
}

/// [`apply_change`] for a set: it also keeps the value it replaces, so a save that fails
/// later can restore it (module docs).
fn apply_backed_up(change: &SecretChange, plan: &SavePlan) -> Result<AppliedChange, SecretError> {
    match plan.secret_store.as_ref() {
        Some(secret_store) => AppliedChange::apply(change, secret_store),
        None => Err(SecretError::BackendUnavailable {
            reason: secret_values::NOT_CONNECTED.to_string(),
        }),
    }
}

/// Put the keychain's applied sets back after a failure (module docs), and say which keys
/// could not go back. With no store connected every one of them counts as unrestorable.
fn rollback_applied(plan: &SavePlan, applied: &[AppliedChange]) -> Vec<SecretKey> {
    match plan.secret_store.as_ref() {
        Some(secret_store) => secret_values::rollback(secret_store, applied),
        None => applied.iter().map(|change| change.key().clone()).collect(),
    }
}

/// The local values part of one save: the clears first, then the writes, counting what
/// lands so a failure partway through can say so (module docs).
fn write_locals(
    plan: &SavePlan,
    store: &Mutex<LocalStore>,
    touched: &mut usize,
) -> anyhow::Result<()> {
    let store_guard = lock_store(store)?;
    for key in &plan.collected.local_clears {
        store_guard
            .local_values()
            .delete(&plan.workspace_id, &plan.scope_id, key)?;
        *touched += 1;
    }
    for local in &plan.collected.local_writes {
        store_guard.local_values().insert(local)?;
        *touched += 1;
    }
    Ok(())
}

/// What [`landed_note`] says about the keychain after a failure rolled the sets back
/// (module docs): every overwritten value restored, or the variable keys — never the
/// values — whose restore failed.
fn keychain_note(restores_failed: &[SecretKey]) -> String {
    if restores_failed.is_empty() {
        KEYCHAIN_RESTORED_NOTE.to_string()
    } else {
        let keys = restores_failed
            .iter()
            .map(|key| format!("`{}`", key.variable_key()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{KEYCHAIN_MAY_HOLD_NOTE}{keys}")
    }
}

/// What the failure message says landed and what was undone (module docs): every message
/// is built from exactly this — never a claim about a part that did not run. It carries
/// keys and counts only, never a value.
fn landed_note(keychain: Option<String>, locals: Option<String>) -> String {
    match (keychain, locals) {
        (None, None) => "Nothing was saved".to_string(),
        (Some(keychain), None) => keychain,
        (None, Some(locals)) => locals,
        (Some(keychain), Some(locals)) => format!("{keychain} and {locals}"),
    }
}

/// The store handle: a poisoned lock is an error, never a panic (the shape
/// `selector::persist_selection` uses for the same shared store).
fn lock_store(store: &Mutex<LocalStore>) -> anyhow::Result<MutexGuard<'_, LocalStore>> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))
}

/// Wire the editor to `env_editor::Open` and `environment::EditSecret` (S43c): open it
/// over the active workspace with the [`EnvEditorInputs`] the app's wire installed, the
/// second with the cursor in the secret row it names.
pub fn init(cx: &mut App) {
    cx.on_action(open_env_editor);
    cx.on_action(open_env_editor_on_secret);
}

/// `env_editor::Open`: the editor with no particular row to aim at.
fn open_env_editor(_: &Open, cx: &mut App) {
    open_editor(None, cx);
}

/// `environment::EditSecret` (S43c): the editor with the cursor in `action.name`'s
/// secret value field — the `UnresolvedSecret` panel's "Set <name>" button's door. The
/// name is a lookup key and is never displayed here ([`NO_SECRET_ROW_MESSAGE`]).
fn open_env_editor_on_secret(action: &EditSecret, cx: &mut App) {
    open_editor(Some(action.name.clone()), cx);
}

/// Open the editor in the active window: first read the scope as it is on disk and in
/// the local store (module docs), off the foreground, then open over it — with the
/// cursor in `focus`'s secret row, when one is named (S43c). The dispatch
/// happens inside the window's own update and a window cannot be updated while it is on
/// the update stack, so the open is deferred — the shape [`super::quick_look`]'s opener
/// uses for the same situation.
fn open_editor(focus: Option<String>, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let prepared = window_handle.update(cx, |root, window, cx| {
            let Ok(workspace_handle) = root.downcast::<Workspace>() else {
                return None;
            };
            // S43c: with an editor already on screen, the open below would toggle it
            // shut ([`toggle_modal`]) — aim the cursor at the row instead.
            if let Some(name) = focus.as_deref()
                && workspace_handle
                    .read(cx)
                    .modal_layer()
                    .active_modal::<EnvEditor>()
                    .is_some()
            {
                workspace_handle.update(cx, |workspace, cx| {
                    aim_at_secret(workspace, name, window, cx)
                });
                return None;
            }
            // Cloned out before the workspace update borrows `cx` again.
            let inputs = match cx.try_global::<EnvEditorInputs>().cloned() {
                Some(inputs) => inputs,
                None => {
                    let workspace = workspace_handle.downgrade();
                    if workspace
                        .update(cx, |workspace, cx| {
                            // Errors reach the user: an editor that cannot save would
                            // edit nothing without saying why.
                            workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        })
                        .is_err()
                    {
                        // Handled: the workspace went away with its window.
                    }
                    return None;
                }
            };
            Some((workspace_handle, inputs))
        });
        let Ok(Some((workspace_handle, inputs))) = prepared else {
            // Handled: the window closed between the keystroke and this callback, or
            // there was nothing wired to open (the toast above already said so).
            return;
        };
        let workspace = workspace_handle.downgrade();
        // The keychain the secret rows' values live in (module docs), read on the
        // foreground and carried into the read that runs off it.
        let secret_store = secret_values::store(cx);
        // Shadowed into the task so the read runs against its own copies.
        let workspace_root = inputs.workspace_root.clone();
        let target = inputs.target.clone();
        let workspace_id = inputs.workspace_id.clone();
        let captured_locals = inputs.local_values.clone();
        let store = Arc::clone(&inputs.store);
        cx.spawn(async move |cx| {
            let opened = cx
                .background_spawn(async move {
                    read_scope(
                        &workspace_root,
                        &target,
                        &workspace_id,
                        captured_locals,
                        &store,
                        secret_store.as_ref(),
                    )
                })
                .await;
            if window_handle
                .update(cx, |_, window, cx| {
                    workspace_handle.update(cx, |workspace_handle, cx| {
                        workspace_handle.toggle_modal(window, cx, |window, cx| {
                            EnvEditor::new(inputs, opened, workspace, window, cx)
                        });
                        if let Some(name) = focus {
                            aim_at_secret(workspace_handle, &name, window, cx);
                        }
                    });
                })
                .is_err()
            {
                // Handled: the window closed while the scope was being read; there is
                // no workspace left to open the editor over.
            }
        })
        .detach();
    });
}

/// Put the cursor in `name`'s secret value field in the editor just opened (S43c). A
/// name this scope has no secret row for is said out loud — the "Set <name>" button
/// promised this row (AGENTS.md: errors reach the user) — with a message that names
/// neither the key nor any value: the button that dispatched this already names the key,
/// and a crafted key never reaches a message (AGENTS.md › Security).
fn aim_at_secret(
    workspace: &mut Workspace,
    name: &str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(editor) = workspace.modal_layer().active_modal::<EnvEditor>() else {
        // The editor did not open; its own opener has already said why (module docs).
        return;
    };
    let Some(field) = editor.read(cx).secret_value_field(name) else {
        workspace.show_toast(NO_SECRET_ROW_MESSAGE, Severity::Error, None, cx);
        return;
    };
    let handle = field.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
}
