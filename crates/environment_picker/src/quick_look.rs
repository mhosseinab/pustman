//! The environment quick-look (PRD REQ-VAR-SHARE-01/02): a dialog listing the active
//! environment's and the globals' variables as the three columns REQ-VAR-SHARE-01 names
//! — key, shared value, local value. A secret carries no shared value (REQ-VAR-SHARE-02)
//! and its local value is masked: [`MASK`] while a keychain entry is set, the "unset"
//! label when it is not — the unresolved secret of REQ-VAR-SHARE-02, never an empty
//! string — and the fetched value only while the user holds the row's reveal toggle on.
//!
//! `Decision:` the surface is the workspace's modal ([`Workspace::toggle_modal`], the
//! same shape `selector.rs` opens its picker with), not a new popover primitive: the
//! step's "popover" is a lightweight overlay, and the modal layer already owns overlay
//! placement, focus trapping, Escape and click-outside dismissal.
//!
//! `Decision:` [`QuickLookInputs`] is an app-global the app's wire installs, the same
//! "the top crate owns the inputs" shape as [`super::SelectorInputs`]: one app-wide
//! [`Global`](gpui::Global) shared by every window, read when the action fires. This crate
//! reaches neither the app's `AppStore` nor the workspace file, and a quick-look opened
//! before anything is installed says so in an error toast instead of listing silence.
//!
//! `Decision:` the variable scope id is the first place the convention is fixed, and it
//! is the same string for `storage::LocalValue` rows and [`SecretKey`]s: the
//! environment's [`api_model::ItemId`] rendering for an environment's scope
//! (stable across renames) and [`GLOBALS_SCOPE_ID`] for the globals. ULID ids carry no
//! `/`, so every key stays representable in the OS keychain.
//!
//! `Decision:` the keychain is read only on background tasks, in two places with two
//! different payloads. The open-time lookup fetches *status* only — `Ok` becomes the
//! mask and the value is dropped right there — so the plaintext never crosses into the
//! view unless the user asks. The reveal toggle runs its own lookup and shows the value
//! it fetched; hiding the row cancels an in-flight fetch and drops the value. A
//! revealed value lands in no tooltip, element id or debug string (AGENTS.md ›
//! Security): the cell truncates without a tooltip, which is the one truncation in this
//! dialog that does not carry its full text.

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{Environment, MASK, Variable, VariableKind};
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, Global, Render, Task, Window,
};
use pustman_actions::environment::QuickLook as QuickLookAction;
use secrets::{SecretError, SecretKey, SecretStore, SecretValue};
use storage::LocalValue;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

#[cfg(test)]
#[path = "quick_look_tests.rs"]
mod tests;

/// The label of a secret with no keychain entry: REQ-VAR-SHARE-02's "unresolved
/// secret" marker, which is a label — never an empty cell and never the mask.
const UNSET_LABEL: &str = "unset";

/// What a cell shows while its keychain lookup is still in flight: the answer is
/// security-relevant, so the cell says "unknown yet" rather than guessing "unset".
const CHECKING_LABEL: &str = "…";

/// What a cell shows when there is no value to show: a non-secret variable with no
/// local value, and the shared value a secret never has.
const NO_VALUE_LABEL: &str = "—";

/// The label of the globals block and the scope id globals are stored under (see the
/// module docs): `"globals"` is separator-free like every ULID id.
const GLOBALS_LABEL: &str = "Globals";

/// The scope id globals are stored under in `local_values` and the OS keychain.
const GLOBALS_SCOPE_ID: &str = "globals";

/// The one message the opener shows when the app has not installed [`QuickLookInputs`]
/// (AGENTS.md: errors reach the user, never only a log).
const NOT_WIRED_MESSAGE: &str =
    "The environment quick-look cannot show variables — the app's stores are not connected";

/// The prefix every keychain failure is shown with, so a row's inline error says what
/// happened and where, not just a platform string.
const READ_FAILURE_PREFIX: &str = "Could not read this secret — ";

/// What the quick-look cannot know on its own: the active environment, the globals and
/// the two stores the local column comes from. See the module docs for the app-global
/// caveat this shape carries.
#[derive(Clone)]
pub struct QuickLookInputs {
    /// The workspace the variables and secrets belong to — part of every [`SecretKey`].
    pub workspace_id: String,
    /// The active environment to list, when one is active (REQ-VAR-08's "No environment"
    /// state simply has none).
    pub environment: Option<Environment>,
    /// The globals to list under the environment (the widest scope, REQ-VAR-02).
    pub globals: Vec<Variable>,
    /// The non-secret local values of both scopes, matched by scope id and key.
    pub local_values: Vec<LocalValue>,
    /// The keychain the secret rows' local values come from.
    pub secret_store: Arc<dyn SecretStore>,
}

impl Global for QuickLookInputs {}

/// Install the inputs the opener runs with; the next `environment::QuickLook` uses them.
pub fn install_inputs(cx: &mut App, inputs: QuickLookInputs) {
    cx.set_global(inputs);
}

/// What a row's local-value cell shows and why.
enum LocalValueDisplay {
    /// A non-secret variable's local value, or [`NO_VALUE_LABEL`] when it has none.
    Local(String),
    /// A secret whose keychain lookup has not answered yet ([`CHECKING_LABEL`]).
    Checking,
    /// A secret with no keychain entry ([`UNSET_LABEL`]).
    Unset,
    /// A secret set in the keychain: the mask until the user reveals it.
    Masked,
    /// A revealed secret: the value the reveal's lookup fetched. Held only while the
    /// row is revealed; hiding the row drops it.
    Revealed(SecretValue),
    /// The lookup failed for a reason other than "not found": shown inline, never
    /// disguised as "unset".
    Error(String),
}

/// One variable row: key, shared value and local value (REQ-VAR-SHARE-01).
struct Row {
    /// Stable element-id base: scope id plus variable key — never a list index (focus
    /// would break on reorder) and never anything that could carry a value.
    id: String,
    /// The scope the row's local value and secret live under (see the module docs).
    scope_id: String,
    key: String,
    /// The shared column's text: the variable's shared value, or [`NO_VALUE_LABEL`] —
    /// for a secret it is always the latter, even if an input smuggles a value in.
    shared_text: String,
    local: LocalValueDisplay,
    /// The reveal toggle's focus handle (secret rows only): what makes the toggle a tab
    /// stop of this dialog and what the keyboard reaches it with.
    reveal_focus: Option<FocusHandle>,
}

impl Row {
    /// The exact string the local column draws.
    fn local_text(&self) -> &str {
        match &self.local {
            LocalValueDisplay::Local(value) => value,
            LocalValueDisplay::Checking => CHECKING_LABEL,
            LocalValueDisplay::Unset => UNSET_LABEL,
            LocalValueDisplay::Masked => MASK,
            LocalValueDisplay::Revealed(value) => value.expose(),
            LocalValueDisplay::Error(message) => message,
        }
    }
}

/// One scope's block: its label and its variable rows, in file order.
struct Block {
    /// The scope's label: the environment's name or [`GLOBALS_LABEL`].
    label: String,
    /// The scope id the rows' values live under (see the module docs).
    scope_id: String,
    rows: Vec<Row>,
}

/// One row as the user sees it: exactly the three strings [`Render`] puts in its cells,
/// so a test can assert on the display without re-deriving it. Carries no `Debug` on
/// purpose — [`DisplayedRow::local`] can hold a revealed secret, and debug strings must
/// never hold one.
pub struct DisplayedRow {
    pub key: String,
    pub shared: String,
    pub local: String,
}

/// The quick-look dialog: the variables of the active environment and the globals, and
/// the masked, "unset" and revealed states of the secret rows.
pub struct QuickLook {
    focus_handle: FocusHandle,
    workspace_id: String,
    blocks: Vec<Block>,
    secret_store: Arc<dyn SecretStore>,
    /// The open-time status lookup. Stored rather than detached so dismissing the
    /// dialog drops — and so cancels — a lookup that is still running (AGENTS.md ›
    /// Dropping a `Task` cancels it).
    status_lookup: Option<Task<()>>,
    /// The in-flight reveal lookups, by row id. Dropping an entry cancels its fetch:
    /// a row that was hidden is never fetched back into the view.
    reveals: BTreeMap<String, Task<()>>,
}

impl QuickLook {
    /// A quick-look over `inputs`, with every secret row's status lookup already
    /// started (the keychain is read on a background task — see the module docs).
    pub fn new(inputs: QuickLookInputs, cx: &mut Context<Self>) -> Self {
        let QuickLookInputs {
            workspace_id,
            environment,
            globals,
            local_values,
            secret_store,
        } = inputs;
        let mut blocks = Vec::new();
        if let Some(environment) = &environment {
            let block = build_block(
                environment.name.clone(),
                environment.id.to_string(),
                &environment.variables,
                &local_values,
                cx,
            );
            if !block.rows.is_empty() {
                blocks.push(block);
            }
        }
        let globals_block = build_block(
            GLOBALS_LABEL.to_string(),
            GLOBALS_SCOPE_ID.to_string(),
            &globals,
            &local_values,
            cx,
        );
        if !globals_block.rows.is_empty() {
            blocks.push(globals_block);
        }
        let mut quick_look = Self {
            focus_handle: cx.focus_handle(),
            workspace_id,
            blocks,
            secret_store,
            status_lookup: None,
            reveals: BTreeMap::new(),
        };
        quick_look.status_lookup = Some(quick_look.start_status_lookup(cx));
        quick_look
    }

    /// Every row as the user sees it, in display order.
    pub fn displayed_rows(&self) -> Vec<DisplayedRow> {
        self.blocks
            .iter()
            .flat_map(|block| block.rows.iter())
            .map(|row| DisplayedRow {
                key: row.key.clone(),
                shared: row.shared_text.clone(),
                local: row.local_text().to_string(),
            })
            .collect()
    }

    /// The reveal control of the first row for `variable_key`, as the keyboard reaches
    /// it: the dialog hands the control its tab order through this handle.
    pub fn reveal_focus_handle(&self, variable_key: &str) -> Option<FocusHandle> {
        self.blocks
            .iter()
            .flat_map(|block| block.rows.iter())
            .find(|row| row.key == variable_key)
            .and_then(|row| row.reveal_focus.clone())
    }

    /// The secret rows' (row id, scope id, key) triples — everything a keychain lookup
    /// needs and nothing that could carry a value.
    fn secret_rows(&self) -> Vec<(String, String, String)> {
        self.blocks
            .iter()
            .flat_map(|block| block.rows.iter())
            .filter(|row| row.reveal_focus.is_some())
            .map(|row| (row.id.clone(), row.scope_id.clone(), row.key.clone()))
            .collect()
    }

    /// Ask the keychain, off the foreground thread, whether each secret row has an
    /// entry. Only the answer crosses back — the value is dropped inside the lookup
    /// (module docs), so the mask's "something is set here" claim costs no plaintext.
    fn start_status_lookup(&self, cx: &mut Context<Self>) -> Task<()> {
        let workspace_id = self.workspace_id.clone();
        let secret_rows = self.secret_rows();
        let store = Arc::clone(&self.secret_store);
        cx.spawn(async move |this, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move {
                    secret_rows
                        .into_iter()
                        .map(|(row_id, scope_id, variable_key)| {
                            let display = lookup_secret_status(
                                &store,
                                &workspace_id,
                                &scope_id,
                                &variable_key,
                            );
                            (row_id, display)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    for (row_id, display) in outcomes {
                        this.apply_local_display(&row_id, display);
                    }
                    cx.notify();
                })
                .is_err()
            {
                // Handled: the dialog went away with its window; its rows are gone.
            }
        })
    }

    /// Turn the row's reveal toggle: on runs the reveal's keychain lookup on a
    /// background task, off drops the row back to the mask and cancels any fetch that
    /// has not landed yet.
    fn set_revealed(&mut self, row_id: &str, revealed: bool, cx: &mut Context<Self>) {
        if !revealed {
            self.reveals.remove(row_id);
            if let Some(row) = self.row_mut(row_id)
                && matches!(row.local, LocalValueDisplay::Revealed(_))
            {
                row.local = LocalValueDisplay::Masked;
            }
            cx.notify();
            return;
        }
        let Some(row) = self.row_mut(row_id) else {
            return;
        };
        // While the fetch is in flight the cell says so and the toggle stays off: the
        // reveal is not on until the value is on screen.
        row.local = LocalValueDisplay::Checking;
        // The row's label and scope name, cloned out before the task borrows nothing.
        let scope_id = row.scope_id.clone();
        let variable_key = row.key.clone();
        let workspace_id = self.workspace_id.clone();
        let store = Arc::clone(&self.secret_store);
        let task_row_id = row_id.to_string();
        let task = cx.spawn(async move |this, cx| {
            let display = cx
                .background_executor()
                .spawn(async move {
                    lookup_secret_value(&store, &workspace_id, &scope_id, &variable_key)
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.reveals.remove(&task_row_id);
                    this.apply_local_display(&task_row_id, display);
                    cx.notify();
                })
                .is_err()
            {
                // Handled: the dialog went away; the fetched value goes with it.
            }
        });
        self.reveals.insert(row_id.to_string(), task);
        cx.notify();
    }

    /// Put `display` in the row's local cell, if the row is still a secret row.
    fn apply_local_display(&mut self, row_id: &str, display: LocalValueDisplay) {
        if let Some(row) = self.row_mut(row_id)
            && row.reveal_focus.is_some()
        {
            row.local = display;
        }
    }

    /// The row with this id, whichever block holds it.
    fn row_mut(&mut self, row_id: &str) -> Option<&mut Row> {
        self.blocks
            .iter_mut()
            .flat_map(|block| block.rows.iter_mut())
            .find(|row| row.id == row_id)
    }

    /// The one block's table: the three REQ-VAR-SHARE-01 columns over the scope's rows.
    fn render_block(&self, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let table = block.rows.iter().fold(
            Table::new(ElementId::from(format!("quick-look-{}", block.scope_id)))
                .column(TableColumn::new("Key"))
                .column(TableColumn::new("Shared value"))
                .column(TableColumn::new("Local value")),
            |table, row| table.row(self.render_row(row, cx)),
        );
        v_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                Headline::new(block.label.clone())
                    .size(HeadlineSize::XSmall)
                    .color(Color::Muted),
            )
            // A big environment scrolls inside the dialog instead of pushing the
            // window's bounds; the scroll id is the stable scope id, never a row index.
            .child(
                div()
                    .id(format!("quick-look-scroll-{}", block.scope_id))
                    .max_h(px_to_rems(320.))
                    .overflow_y_scroll()
                    .child(table),
            )
            .into_any_element()
    }

    /// One row's cells: key, shared value and local value — the last one wearing the
    /// reveal toggle when the variable is a secret.
    fn render_row(&self, row: &Row, cx: &mut Context<Self>) -> TableRow {
        let local_text = row.local_text().to_string();
        let local_cell = match &row.reveal_focus {
            Some(focus_handle) => {
                let reveal_row_id = row.id.clone();
                let reveal = cx.weak_entity();
                let state = if matches!(row.local, LocalValueDisplay::Revealed(_)) {
                    ToggleState::Selected
                } else {
                    ToggleState::Unselected
                };
                let revealable = matches!(
                    row.local,
                    LocalValueDisplay::Masked | LocalValueDisplay::Revealed(_)
                );
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    // A revealed secret truncates with no tooltip: the tooltip text
                    // would outlive the reveal and leak into hover surfaces.
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(Label::new(local_text).truncate()),
                    )
                    .child(
                        Toggle::new(
                            ElementId::from(format!("reveal-{}", row.id)),
                            "Reveal",
                            state,
                        )
                        .focus_handle(focus_handle.clone())
                        .disabled(!revealable)
                        .on_change(move |toggle_state, _window, cx| {
                            if reveal
                                .update(cx, |this, cx| {
                                    this.set_revealed(
                                        &reveal_row_id,
                                        toggle_state.is_selected(),
                                        cx,
                                    );
                                })
                                .is_err()
                            {
                                // Handled: the dialog went away between click and
                                // callback; there is no row left to reveal.
                            }
                        }),
                    )
                    .into_any_element()
            }
            None => value_cell(ElementId::from(format!("{}-local", row.id)), local_text),
        };
        TableRow::new(ElementId::from(row.id.clone()))
            .cell(value_cell(
                ElementId::from(format!("{}-key", row.id)),
                row.key.clone(),
            ))
            .cell(value_cell(
                ElementId::from(format!("{}-shared", row.id)),
                row.shared_text.clone(),
            ))
            .cell(local_cell)
    }
}

/// A value cell: truncating text whose tooltip carries the full value. Only text that
/// cannot hold a secret may land here — a secret row's local cell is built by hand
/// without a tooltip.
fn value_cell(id: ElementId, text: String) -> AnyElement {
    div()
        .id(id)
        .min_w_0()
        .flex_1()
        .tooltip(Tooltip::text(text.clone()).builder())
        .child(Label::new(text).truncate())
        .into_any_element()
}

/// Build one scope's block: one row per variable, secrets starting out
/// [`LocalValueDisplay::Checking`] and non-secret rows carrying their local value.
fn build_block(
    label: String,
    scope_id: String,
    variables: &[Variable],
    local_values: &[LocalValue],
    cx: &mut Context<QuickLook>,
) -> Block {
    let rows = variables
        .iter()
        .map(|variable| {
            let is_secret = variable.kind == VariableKind::Secret;
            let local = if is_secret {
                LocalValueDisplay::Checking
            } else {
                LocalValueDisplay::Local(
                    local_values
                        .iter()
                        .find(|value| value.scope_id == scope_id && value.key == variable.key)
                        .map(|value| value.value.clone())
                        .unwrap_or_else(|| NO_VALUE_LABEL.to_string()),
                )
            };
            // A secret never has a shared value, whatever an import smuggled into the
            // field (REQ-VAR-SHARE-02): the shared column shows nothing.
            let shared_text = match (is_secret, variable.value.as_deref()) {
                (true, _) | (false, None) => NO_VALUE_LABEL.to_string(),
                (false, Some(value)) => value.to_string(),
            };
            Row {
                id: format!("{scope_id}:{}", variable.key),
                scope_id: scope_id.clone(),
                key: variable.key.clone(),
                shared_text,
                local,
                reveal_focus: is_secret.then(|| cx.focus_handle()),
            }
        })
        .collect();
    Block {
        label,
        scope_id,
        rows,
    }
}

/// One keychain *status* lookup for a secret row: is an entry set? Runs off the
/// foreground thread and never returns the value — `Ok` is folded to the mask where the
/// plaintext is dropped (module docs).
fn lookup_secret_status(
    store: &Arc<dyn SecretStore>,
    workspace_id: &str,
    scope_id: &str,
    variable_key: &str,
) -> LocalValueDisplay {
    let secret_key = match SecretKey::new(workspace_id, scope_id, variable_key) {
        Ok(secret_key) => secret_key,
        Err(error) => return LocalValueDisplay::Error(read_failure(&error)),
    };
    match store.get(&secret_key) {
        Ok(_value) => LocalValueDisplay::Masked,
        Err(SecretError::NotFound { .. }) => LocalValueDisplay::Unset,
        Err(error) => LocalValueDisplay::Error(read_failure(&error)),
    }
}

/// One keychain *value* lookup, behind a reveal: the plaintext this returns lives in
/// the view only while the row is revealed. Same errors as
/// [`lookup_secret_status`] — a secret deleted between the two lookups becomes
/// "unset", never an empty reveal.
fn lookup_secret_value(
    store: &Arc<dyn SecretStore>,
    workspace_id: &str,
    scope_id: &str,
    variable_key: &str,
) -> LocalValueDisplay {
    let secret_key = match SecretKey::new(workspace_id, scope_id, variable_key) {
        Ok(secret_key) => secret_key,
        Err(error) => return LocalValueDisplay::Error(read_failure(&error)),
    };
    match store.get(&secret_key) {
        Ok(value) => LocalValueDisplay::Revealed(value),
        Err(SecretError::NotFound { .. }) => LocalValueDisplay::Unset,
        Err(error) => LocalValueDisplay::Error(read_failure(&error)),
    }
}

/// The row's inline error for a keychain failure: what happened and on which key, so
/// the message is actionable (AGENTS.md › Errors must reach the user). The error names
/// the key, never a value.
fn read_failure(error: &SecretError) -> String {
    format!("{READ_FAILURE_PREFIX}{error}")
}

impl Render for QuickLook {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base08.rems(cx))
            .when(self.blocks.is_empty(), |content| {
                // The empty state names the next action instead of an empty table.
                content.child(Label::new(
                    "No variables yet — add them to the environment or the globals.",
                ))
            })
            .children(self.blocks.iter().map(|block| self.render_block(block, cx)));
        Modal::new("Environment quick-look")
            .content(content)
            .footer(
                Button::new("quick-look-close", "Close").on_click(cx.listener(
                    |_this, _event, _window, cx| {
                        cx.emit(DismissEvent);
                    },
                )),
            )
    }
}

impl Focusable for QuickLook {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for QuickLook {}

impl ModalView for QuickLook {}

/// Wire the quick-look to `environment::QuickLook`: open it over the active workspace
/// with the [`QuickLookInputs`] the app's wire installed.
pub fn init(cx: &mut App) {
    cx.on_action(open_quick_look);
}

/// Open the quick-look in the active window. The dispatch happens inside the window's
/// own update and a window cannot be updated while it is on the update stack, so the
/// open is deferred — the shape `selector`'s opener uses for the same situation.
fn open_quick_look(_: &QuickLookAction, cx: &mut App) {
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
                let inputs = cx.try_global::<QuickLookInputs>().cloned();
                workspace_handle.update(cx, |workspace, cx| {
                    let Some(inputs) = inputs else {
                        // Errors reach the user: a quick-look that cannot read the
                        // stores would list nothing without saying why.
                        workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        return;
                    };
                    workspace.toggle_modal(window, cx, |_window, cx| QuickLook::new(inputs, cx));
                });
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is no
            // workspace left to open the quick-look over.
        }
    });
}
