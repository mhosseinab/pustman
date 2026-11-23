//! `history_panel` — the left-dock History panel (PRD §8.5, REQ-HIST-01..04): one
//! workspace's recorded sends, grouped by day in a virtualized list with a search box
//! (REQ-HIST-01, REQ-HIST-04). The rows carry method, target, status and time only —
//! never a request or response body (REQ-HIST-03). Opening an entry and deleting
//! entries are here ([`open_entry`], [`delete_entries`]); "Save to collection" is a
//! later step.
//!
//! Doors in:
//! - [`history_list`] — the [`HistoryList`] panel and the rows it draws.
//! - [`open_entry`] — one recorded send comes back as a request tab (REQ-HIST-02).
//! - [`delete_entries`] — deleting one entry, and "Clear all" behind its confirm modal
//!   (REQ-HIST-04).
//! - [`init`] — the app-level wiring this crate owns: `history::Focus` moves keyboard
//!   focus to the panel (plan D10: feature crates implement their action handlers), and
//!   the delete key the list answers on its keyboard selection.

pub mod history_list;

pub use history_list::{HistoryList, HistoryRow};

use gpui::{App, KeyBinding};
use pustman_actions::history::Focus;
use workspace::Workspace;

use crate::history_list::{LIST_KEY_CONTEXT, PANEL_ID};

/// The panel's own delete operations (PRD REQ-HIST-04), answered by [`HistoryList`].
///
/// `Decision:` they are declared here, not in `pustman_actions::history`: that crate's
/// test pins the `history::` namespace to exactly `Focus`, and its keymap loader resolves
/// the bundled JSON in a binary that does not link this crate — so the binding lives in
/// [`init`] and the actions under this crate's own namespace.
pub mod actions {
    gpui::actions!(
        history_panel,
        [
            /// Deletes the history entry the keyboard selection is on.
            DeleteEntry,
            /// Asks to confirm, then removes every history entry of the workspace.
            ClearHistory,
        ]
    );
}

/// The keystroke that deletes the selected entry. Plain Delete/Backspace belong to the
/// search box, which holds the panel's keyboard focus, so the binding takes the
/// platform's "delete the item" chord instead (Finder's Cmd-Backspace; Shift-Delete
/// elsewhere) — neither edits text in `text_input`.
const DELETE_ENTRY_KEYSTROKE: &str = if cfg!(target_os = "macos") {
    "cmd-backspace"
} else {
    "shift-delete"
};

/// Wire the panel's app-level action (D10): `history::Focus` puts the keyboard in the
/// history panel from wherever the user is (PRD §8.5). Also binds the list's delete key
/// in the list's key context.
pub fn init(cx: &mut App) {
    cx.on_action(focus_history_panel);
    cx.bind_keys([KeyBinding::new(
        DELETE_ENTRY_KEYSTROKE,
        actions::DeleteEntry,
        Some(LIST_KEY_CONTEXT),
    )]);
}

/// Show the history panel and move keyboard focus into it — `Dock::activate_panel` does
/// both. A window whose left dock holds no history panel (the wire step has not run
/// yet) does nothing: the dock ignores an id it does not know.
fn focus_history_panel(_: &Focus, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    // Deferred like `command_palette`'s toggle: the dispatch runs inside the window's
    // own update, and a window cannot be updated while it is on the update stack.
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace
                        .left_dock()
                        .update(cx, |dock, cx| dock.activate_panel(PANEL_ID, window, cx));
                });
            })
            .is_err()
        {
            // Handled: the window closed between the keystroke and this callback; there
            // is no panel left to focus and nothing to tell anyone about.
        }
    });
}

/// Delete one entry or clear the workspace's history (PRD REQ-HIST-04): the pub
/// operations in [`delete_entries`], with "Clear all" behind its confirm modal.
pub mod delete_entries;

/// Opening a history entry (PRD REQ-HIST-02): [`open_entry::open_entry`] restores the
/// stored pre-resolution request into a request tab and shows the recorded response in
/// that tab's response region.
pub mod open_entry;
