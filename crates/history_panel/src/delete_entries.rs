//! Deleting history (PRD REQ-HIST-04): one recorded send by its stored id, and "Clear
//! all" — which sits behind a confirm modal, because it removes every entry the
//! workspace ever recorded (pack screen S13).
//!
//! `storage` is synchronous (ADR-0002), so the deletes run on a background task and the
//! foreground never touches SQLite. A successful delete refreshes the [`HistoryList`]
//! through its `pub(crate)` reload; a failure becomes an error toast on the
//! [`Workspace`] (AGENTS.md — errors reach the user, never only a log).
//!
//! `Decision:` the operations are pub methods on [`HistoryMaintenance`], not
//! `pustman_actions` entries: `pustman_actions::history` holds only `Focus` and this
//! step may not edit that crate — history delete/clear actions are not yet in
//! `pustman_actions`. The step that draws the row's delete button and the panel's
//! "Clear all" button wires them to these methods.

use std::sync::{Arc, Mutex};

use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, WeakEntity, Window,
    div,
};
use storage::LocalStore;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

use crate::history_list::HistoryList;

#[cfg(test)]
#[path = "delete_entries_tests.rs"]
mod tests;

/// The one message a failed single delete shows. `storage`'s error text can quote the
/// row it failed on, and rows are request content (REQ-HIST-03) — the failure is folded
/// into this fixed message and the error is dropped unread (the shape
/// [`crate::history_list`] reads under).
const DELETE_FAILURE_MESSAGE: &str = "The history entry could not be deleted";

/// The one message a failed "Clear all" shows, folded the same way.
const CLEAR_FAILURE_MESSAGE: &str = "The request history could not be cleared";

/// The confirm modal's heading: it names the destructive operation exactly, in the
/// panel's own terminology ("History", "requests").
const CLEAR_CONFIRM_TITLE: &str = "Clear all history?";

/// What the confirm modal warns about before Confirm: scope and irreversibility, the
/// two things a destructive confirm must say (ux-checklist: destructive actions
/// confirm).
const CLEAR_CONFIRM_BODY: &str =
    "This removes every request this workspace recorded. It cannot be undone.";

/// The Cancel button's test hook ([`ui::components::list::VirtualList`]'s pattern: a
/// stable selector beside the element id).
const CANCEL_BUTTON_SELECTOR: &str = "history-clear-cancel";

/// The Confirm button's test hook.
const CONFIRM_BUTTON_SELECTOR: &str = "history-clear-confirm";

/// The delete-and-clear operations of one workspace's history: what a row's delete
/// button and the panel's "Clear all" will run. It carries the store and the workspace
/// the rows belong to, plus weak references to the list to refresh and the workspace to
/// report errors on — so the operations never depend on a view borrow that ends at the
/// click.
#[derive(Clone)]
pub struct HistoryMaintenance {
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
    list: WeakEntity<HistoryList>,
    workspace: WeakEntity<Workspace>,
}

impl HistoryMaintenance {
    /// The operations over `store`'s `workspace_id` history, refreshing `list` and
    /// reporting failures to `workspace`.
    pub fn new(
        store: Arc<Mutex<LocalStore>>,
        workspace_id: impl Into<String>,
        list: &Entity<HistoryList>,
        workspace: &Entity<Workspace>,
    ) -> Self {
        Self {
            store,
            workspace_id: workspace_id.into(),
            list: list.downgrade(),
            workspace: workspace.downgrade(),
        }
    }

    /// Delete one recorded send by its stored id — the entry's stable identity, never
    /// its row position, which shifts as the list re-groups.
    pub fn delete_entry(&self, entry_id: i64, cx: &mut App) {
        let workspace_id = self.workspace_id.clone();
        self.run(cx, DELETE_FAILURE_MESSAGE, move |store| {
            store.history().delete(&workspace_id, entry_id)?;
            Ok(())
        });
    }

    /// "Clear all": open [`ClearHistoryModal`] over `workspace`. Nothing is deleted
    /// until the modal's Confirm runs — Cancel, Escape and a click outside the dialog
    /// all leave the history intact (REQ-HIST-04).
    pub fn clear_all(&self, window: &mut Window, cx: &mut App) {
        let maintenance = self.clone();
        if self
            .workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, |_, cx| ClearHistoryModal::new(maintenance, cx));
            })
            .is_err()
        {
            // Handled: the workspace went away with its window; there is no modal to
            // open and nothing to tell anyone about.
        }
    }

    /// What the confirm modal's Confirm runs: remove every entry this workspace
    /// recorded. Private because the modal is the only door — a bare wipe must not be
    /// one method call away from a stray caller.
    fn clear_every_entry(&self, cx: &mut App) {
        let workspace_id = self.workspace_id.clone();
        self.run(cx, CLEAR_FAILURE_MESSAGE, move |store| {
            store.history().clear(&workspace_id)?;
            Ok(())
        });
    }

    /// Run one delete off the foreground and land its result: a success refreshes the
    /// list, a failure becomes an error toast on the workspace.
    ///
    /// `Decision:` the task is detached rather than stored in a field: confirming the
    /// modal drops it, and a dropped `Task` would cancel the wipe mid-flight
    /// (AGENTS.md › Dropping a `Task` cancels it). Detaching is only sound with the
    /// task carrying its own error report, which this is: a failed delete becomes an
    /// error toast (the shape `environment_picker`'s persisted write uses).
    fn run(
        &self,
        cx: &mut App,
        failure_message: &'static str,
        work: impl FnOnce(&LocalStore) -> anyhow::Result<()> + Send + 'static,
    ) {
        let store = Arc::clone(&self.store);
        let list = self.list.clone();
        let workspace = self.workspace.clone();
        cx.spawn(async move |cx| {
            // The store lock guards the delete only — it is never held across an
            // `.await` (`.agents/rules/rust-code.md`).
            let outcome = cx
                .background_spawn(async move {
                    let store = store
                        .lock()
                        .map_err(|_| anyhow::anyhow!("the local store lock is poisoned"))?;
                    work(&store)
                })
                .await;
            match outcome {
                Ok(()) => {
                    if list.update(cx, |list, cx| list.reload(cx)).is_err() {
                        // Handled: the panel went away with its window before the
                        // delete landed; there is no list left to refresh.
                    }
                }
                Err(_error) => {
                    if workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_toast(failure_message, Severity::Error, None, cx);
                        })
                        .is_err()
                    {
                        // Handled: the workspace went away with its window; there is no
                        // one left to tell that the delete failed.
                    }
                }
            }
        })
        .detach();
    }
}

/// The confirm dialog behind "Clear all" (PRD REQ-HIST-04): one [`ui::Modal`] with the
/// two things a destructive confirm owes the user — Cancel that changes nothing, and a
/// Confirm that says what it destroys. Escape and a click outside dismiss through the
/// modal layer without clearing.
pub struct ClearHistoryModal {
    maintenance: HistoryMaintenance,
    // Kept on the view (not built in render) so the modal layer can take keyboard focus
    // on open and give it back on dismiss.
    focus_handle: FocusHandle,
}

impl ClearHistoryModal {
    fn new(maintenance: HistoryMaintenance, cx: &mut Context<Self>) -> Self {
        Self {
            maintenance,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Cancel: dismiss without touching the history.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    /// Confirm: the one path that clears, then dismisses.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        self.maintenance.clear_every_entry(cx);
        cx.emit(DismissEvent);
    }
}

impl Render for ClearHistoryModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus_handle).child(
            Modal::new(SharedString::new_static(CLEAR_CONFIRM_TITLE))
                .content(
                    Label::new(SharedString::new_static(CLEAR_CONFIRM_BODY)).size(LabelSize::Small),
                )
                .footer(
                    h_flex()
                        .justify_end()
                        .gap(DynamicSpacing::Base04.rems(cx))
                        // Both buttons are labelled text buttons (their accessible
                        // names) and `ui::Button` keeps each one a tab stop Enter
                        // and Space activate, so the dialog is fully operable from
                        // the keyboard without this crate binding a key.
                        .child(
                            div()
                                .id("cancel-clear-history")
                                .debug_selector(|| CANCEL_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new(
                                        "cancel-clear-history",
                                        SharedString::new_static("Cancel"),
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .on_click(cx.listener(
                                        |this, _event, _window, cx| {
                                            this.cancel(cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .id("confirm-clear-history")
                                .debug_selector(|| CONFIRM_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new(
                                        "confirm-clear-history",
                                        SharedString::new_static("Clear all"),
                                    )
                                    .style(ButtonStyle::Tinted(TintColor::Error))
                                    .on_click(cx.listener(
                                        |this, _event, _window, cx| {
                                            this.confirm(cx);
                                        },
                                    )),
                                ),
                        ),
                ),
        )
    }
}

impl Focusable for ClearHistoryModal {
    /// The modal is one tab stop at the top: its buttons carry the tab order after it.
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for ClearHistoryModal {}

impl ModalView for ClearHistoryModal {}
