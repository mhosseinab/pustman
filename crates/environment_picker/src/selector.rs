//! The active-environment selector (PRD REQ-VAR-08): a [`Picker`] listing the workspace's
//! environments plus the first-class "No environment" option, with the confirmed choice
//! persisted per workspace in `storage`'s `kv` table on a background task.
//!
//! `Decision:` the picker is [`command_palette::Picker`] with this crate's own delegate.
//! S24a built it generic for exactly this reuse (its crate docs name S34a), so there is
//! no second list, filter or keyboard contract here — [`command_palette::fuzzy`] ranks
//! the candidates.
//!
//! `Decision:` the environments and the `Arc<Mutex<LocalStore>>` are inputs (the step
//! brief: no workspace file IO here, and the app store lives in `pustman`, which this
//! crate may not depend on). The `environment::SelectEnvironment` opener reads them from
//! [`SelectorInputs`], which the app's wire installs with [`install_inputs`] — the shape
//! `pustman::restore_wiring` uses for the same "the top crate owns the inputs" problem.
//! A picker opened before anything is installed says so in an error toast instead of
//! silently losing the user's choice.
//!
//! `Decision:` the persisted value is the environment's stable [`ItemId`] (renaming an
//! environment does not orphan the selection), and the key carries the workspace id, so
//! two workspaces never share a selection. "No environment" deletes the row — the stored
//! state is one optional value, never a sentinel string.

use std::sync::{Arc, Mutex, MutexGuard};

use api_model::{Environment, ItemId};
use command_palette::fuzzy;
use command_palette::{Picker, PickerDelegate};
use gpui::{App, Context, DismissEvent, ElementId, Global, WeakEntity};
use pustman_actions::environment::SelectEnvironment;
use storage::LocalStore;
use ui::prelude::*;
use workspace::Workspace;

use crate::env_editor;
use crate::environments_panel::create_environment;

#[cfg(test)]
#[path = "selector_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "selector_match_key_tests.rs"]
mod match_key_tests;

#[cfg(test)]
#[path = "selector_menu_tests.rs"]
mod menu_tests;

/// The label of the first-class "No environment" option: picking it clears the stored
/// selection (pack screen S07's "No active environment" state). It is also what the
/// status bar's active-environment readout shows while none is active — one concept, one
/// string.
pub const NO_ENVIRONMENT_LABEL: &str = "No environment";

/// The one message a failed selection write shows (AGENTS.md: errors reach the user,
/// never only a log) — what the user should know: the choice is real for this session.
const WRITE_FAILURE_MESSAGE: &str =
    "The active environment could not be saved — your choice applies until Pustman restarts";

/// The menu's controls: their element ids and test selectors.
pub const NEW_ENVIRONMENT_BUTTON: &str = "environment-menu-new";
pub const EDIT_ENVIRONMENTS_BUTTON: &str = "environment-menu-edit";
const EDIT_ENVIRONMENTS_LABEL: &str = "Edit environments…";

/// The one message the opener shows when the app has not installed [`SelectorInputs`].
const NOT_WIRED_MESSAGE: &str =
    "The environment picker cannot save a selection — the local store is not connected";

/// What the `environment::SelectEnvironment` opener cannot know on its own: the
/// environments (input to this crate — see the module docs) and the app's local store
/// (the `pustman::AppStore` global, unreachable without depending on the top crate).
///
/// One [`Global`] for the whole app, not one per window: the app's wire installs it
/// once per app (every window's opener reads that same copy when the action fires), so
/// replacing it after a change keeps every picker current. `Decision:` it stays
/// app-wide for v1's one workspace (REQ-VAR-08, pack S07/R07); scoping it per window is
/// the follow-up that must land before multi-window (issue #118).
#[derive(Clone)]
pub struct SelectorInputs {
    /// The workspace the selection is scoped to — it is part of the kv key.
    pub workspace_id: String,
    /// The environments to list, in the order the caller wants them shown.
    pub environments: Vec<Environment>,
    /// The currently active environment: preselected and marked in the list.
    pub active: Option<ItemId>,
    /// The app's local store the selection is persisted into.
    pub store: Arc<Mutex<LocalStore>>,
}

impl Global for SelectorInputs {}

/// Install the inputs the opener runs with; the next `environment::SelectEnvironment`
/// uses them.
pub fn install_inputs(cx: &mut App, inputs: SelectorInputs) {
    cx.set_global(inputs);
}

/// One listed option: the first-class "No environment" choice (`id: None`) or one
/// environment. A candidate carries its stable id so its element id never depends on the
/// row's rank, which the query re-ranks.
struct Candidate {
    id: Option<ItemId>,
    name: String,
}

/// The picker behind the selector: the environments plus "No environment",
/// fuzzy-ranked by [`fuzzy`], and the per-workspace persistence of the confirmed choice.
pub struct EnvironmentSelector {
    workspace_id: String,
    store: Arc<Mutex<LocalStore>>,
    workspace: WeakEntity<Workspace>,
    candidates: Vec<Candidate>,
    /// Indices into `candidates`, in rank order — what the list draws.
    matches: Vec<usize>,
    selected: usize,
    active: Option<ItemId>,
}

impl EnvironmentSelector {
    /// A selector over `environments` for `workspace_id`, with `active` preselected and
    /// marked. The store is where the choice is persisted; the workspace is where a
    /// failed write is reported.
    pub fn new(
        workspace_id: impl Into<String>,
        environments: Vec<Environment>,
        active: Option<ItemId>,
        store: Arc<Mutex<LocalStore>>,
        workspace: WeakEntity<Workspace>,
    ) -> Self {
        let mut candidates = vec![Candidate {
            id: None,
            name: NO_ENVIRONMENT_LABEL.to_string(),
        }];
        candidates.extend(environments.into_iter().map(|environment| Candidate {
            id: Some(environment.id),
            name: environment.name,
        }));
        let selected = active
            .as_ref()
            .and_then(|active| {
                candidates
                    .iter()
                    .position(|candidate| candidate.id.as_ref() == Some(active))
            })
            .unwrap_or(0);
        let matches = (0..candidates.len()).collect();
        Self {
            workspace_id: workspace_id.into(),
            store,
            workspace,
            candidates,
            matches,
            selected,
            active,
        }
    }

    /// The candidate behind match `index`, if there is one.
    fn candidate_at(&self, index: usize) -> Option<&Candidate> {
        self.matches
            .get(index)
            .and_then(|candidate_index| self.candidates.get(*candidate_index))
    }

    /// "＋": write a new environment through the sidebar's create path. The write is
    /// detached — the menu closes right after — and carries its own report (a toast
    /// either way).
    fn new_environment(&self, cx: &mut App) {
        if let Some((_name, created)) = create_environment(self.workspace.clone(), cx) {
            created.detach();
        }
    }

    /// Remember `choice` as the active environment and persist it for this workspace on a
    /// background task: the environment's id under [`selection_key`], or — for
    /// "No environment" — no row at all.
    fn select(&mut self, choice: Option<ItemId>, cx: &mut Context<Picker<Self>>) {
        self.active = choice;
        cx.notify();
        persist_choice(
            choice,
            self.workspace_id.clone(),
            Arc::clone(&self.store),
            self.workspace.clone(),
            cx,
        );
    }
}

/// Make `choice` the active environment — what the picker's confirm and the sidebar's
/// Environments list (S48u3) both do: the installed [`SelectorInputs`] take it, so the
/// status bar's readout and every resolver follow, and it is persisted for the inputs'
/// workspace on a background task. Without installed inputs there is nowhere to persist
/// the choice, and the user is told so.
pub fn select_environment(choice: Option<ItemId>, workspace: WeakEntity<Workspace>, cx: &mut App) {
    let Some(inputs) = cx.try_global::<SelectorInputs>().cloned() else {
        if let Err(_released) = workspace.update(cx, |workspace, cx| {
            workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
        }) {
            // Handled: the window is gone; no one is left to tell.
        }
        return;
    };
    persist_choice(choice, inputs.workspace_id, inputs.store, workspace, cx);
}

/// The shared half of a selection: the installed inputs take `choice`, and the write
/// runs detached with its own error report.
///
/// `Decision:` the task is detached rather than stored in a field: confirming
/// dismisses (and drops) the picker, and a dropped `Task` would cancel the write
/// mid-flight (AGENTS.md › Dropping a `Task` cancels it). Detaching is only sound
/// with the task carrying its own error report, which is the other half of this
/// contract: a failed write becomes an error toast on the workspace.
fn persist_choice(
    choice: Option<ItemId>,
    workspace_id: String,
    store: Arc<Mutex<LocalStore>>,
    workspace: WeakEntity<Workspace>,
    cx: &mut App,
) {
    // The confirmed choice replaces what the installed inputs carry, so the status
    // bar's active-environment readout and this picker's own preselection follow the
    // user instead of going stale (REQ-VAR-08). Inputs installed only after a choice
    // is made are not overwritten here.
    if cx.has_global::<SelectorInputs>() {
        cx.global_mut::<SelectorInputs>().active = choice;
    }
    let key = selection_key(&workspace_id);
    cx.spawn(async move |cx| {
        let write = cx
            .background_executor()
            .spawn(async move { persist_selection(&store, &key, choice.as_ref()) })
            .await;
        if write.is_err()
            && let Err(_released) = workspace.update(cx, |workspace, cx| {
                workspace.show_toast(WRITE_FAILURE_MESSAGE, Severity::Error, None, cx);
            })
        {
            // Handled: the workspace went away with its window; no one is left to
            // tell that the write failed.
        }
    })
    .detach();
}

impl PickerDelegate for EnvironmentSelector {
    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>) {
        self.selected = index;
        cx.notify();
    }

    fn update_matches(&mut self, query: &str, cx: &mut Context<Picker<Self>>) {
        self.matches = fuzzy::rank(
            query,
            self.candidates
                .iter()
                .map(|candidate| candidate.name.as_str()),
        )
        .into_iter()
        .map(|string_match| string_match.candidate_index)
        .collect();
        cx.notify();
    }

    fn confirm(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(candidate) = self.candidate_at(self.selected) else {
            return;
        };
        let choice = candidate.id;
        self.select(choice, cx);
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        index: usize,
        _selected: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let Some(candidate) = self.candidate_at(index) else {
            return div().into_any_element();
        };
        let active = self.active == candidate.id;
        h_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            // The current choice is marked before the user changes anything (pack screen
            // S07 shows the active environment), in a fixed slot at the start of the row
            // (mockup section 6) so the names line up whichever row carries it.
            .child(
                div()
                    .flex_none()
                    .w(IconSize::Small.rems())
                    .children(active.then(|| {
                        Icon::new(IconName::Check)
                            .size(IconSize::Small)
                            .color(Color::Accent)
                    })),
            )
            .child(
                div()
                    .id(self.match_key(index))
                    .min_w_0()
                    .flex_1()
                    .tooltip(Tooltip::text(candidate.name.clone()).builder())
                    .child(
                        Label::new(candidate.name.clone())
                            // "No environment" is a choice, not a name (mockup section 6).
                            .color(if candidate.id.is_none() {
                                Color::Muted
                            } else {
                                Color::Default
                            })
                            .truncate(),
                    ),
            )
            .into_any_element()
    }

    fn match_key(&self, index: usize) -> ElementId {
        match self.candidate_at(index) {
            // The environment's stable id — never the row's rank, which the query
            // re-ranks and which would break focus on reorder.
            Some(Candidate {
                id: Some(environment_id),
                ..
            }) => ElementId::from(format!("environment-{environment_id}")),
            Some(Candidate { id: None, .. }) => ElementId::from("environment-none"),
            // No stable model id exists for an out-of-range row — it has no candidate —
            // so the row's own index is the only uniqueness available. It is a fallback
            // the picker never draws (issue #119: unreachable while `match_count` holds),
            // and two such rows must still never share one id.
            None => ElementId::from(format!("environment-out-of-range-{index}")),
        }
    }

    fn placeholder_text(&self) -> SharedString {
        SharedString::new_static("Search environments")
    }

    /// "＋" at the right of the search field (mockup section 6): a new environment
    /// through the sidebar's own create path ([`create_environment`]); the menu closes
    /// and the toast says what was created.
    fn render_query_accessory(
        &self,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        Some(
            div()
                .debug_selector(|| NEW_ENVIRONMENT_BUTTON.to_owned())
                .child(
                    IconButton::new(
                        NEW_ENVIRONMENT_BUTTON,
                        IconName::Plus,
                        Tooltip::text("New environment"),
                    )
                    .on_click(cx.listener(|picker, _, _window, cx| {
                        picker.delegate().new_environment(cx);
                        cx.emit(DismissEvent);
                    })),
                )
                .into_any_element(),
        )
    }

    /// "Edit environments…" under the list (mockup section 6): the variables editor,
    /// through its own `env_editor::Open`.
    fn render_footer(
        &self,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        Some(
            div()
                .debug_selector(|| EDIT_ENVIRONMENTS_BUTTON.to_owned())
                .child(
                    Button::new(EDIT_ENVIRONMENTS_BUTTON, EDIT_ENVIRONMENTS_LABEL)
                        .style(ButtonStyle::Transparent)
                        .full_width()
                        .on_click(cx.listener(|_picker, _, window, cx| {
                            cx.emit(DismissEvent);
                            window.dispatch_action(Box::new(env_editor::Open), cx);
                        })),
                )
                .into_any_element(),
        )
    }
}

/// The kv key the active environment is stored under for one workspace: the workspace id
/// is part of the key (REQ-VAR-08: "stored per workspace locally"), so two workspaces
/// never share a selection.
pub fn selection_key(workspace_id: &str) -> String {
    format!("environment_picker.active_environment.{workspace_id}")
}

/// Write one workspace's selection — the environment's id, or deleting the row for
/// "No environment". Runs off the foreground thread; the store lock is only held across
/// calls that cannot yield (`.agents/rules/rust-code.md`).
fn persist_selection(
    store: &Mutex<LocalStore>,
    key: &str,
    choice: Option<&ItemId>,
) -> anyhow::Result<()> {
    let store_guard = lock_store(store)?;
    match choice {
        Some(environment_id) => store_guard.kv().insert(key, &environment_id.to_string()),
        None => store_guard.kv().delete(key),
    }?;
    Ok(())
}

/// The store handle: a poisoned lock is an error, never a panic (the shape
/// `workspace::restore` uses for the same shared store).
fn lock_store(store: &Mutex<LocalStore>) -> anyhow::Result<MutexGuard<'_, LocalStore>> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))
}

/// Wire the picker to `environment::SelectEnvironment`: open it over the active
/// workspace, listing the environments [`SelectorInputs`] supplies and persisting the
/// choice per workspace.
pub fn init(cx: &mut App) {
    cx.on_action(open_environment_picker);
}

/// Open the picker in the active window. The dispatch happens inside the window's own
/// update and a window cannot be updated while it is on the update stack, so the open is
/// deferred — the shape `command_palette`'s toggle uses for the same situation.
fn open_environment_picker(_: &SelectEnvironment, cx: &mut App) {
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
                let inputs = cx.try_global::<SelectorInputs>().cloned();
                let workspace_reference = workspace_handle.downgrade();
                workspace_handle.update(cx, |workspace, cx| {
                    let Some(inputs) = inputs else {
                        // Errors reach the user: a picker that cannot save the choice
                        // would lose it silently.
                        workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        return;
                    };
                    workspace.toggle_modal(window, cx, |window, cx| {
                        Picker::new(
                            EnvironmentSelector::new(
                                inputs.workspace_id,
                                inputs.environments,
                                inputs.active,
                                inputs.store,
                                workspace_reference,
                            ),
                            window,
                            cx,
                        )
                    });
                });
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is no
            // workspace left to open the picker over and nothing to tell anyone about.
        }
    });
}
