//! `history_panel` — the left-dock History panel (PRD §8.5, REQ-HIST-01..04): one
//! workspace's recorded sends, grouped by day in a virtualized list with a search box
//! (REQ-HIST-01, REQ-HIST-04). The rows carry method, target, status and time only —
//! never a request or response body (REQ-HIST-03). Opening an entry, deleting entries
//! and "Save to collection" are later steps; the list is read-only here.
//!
//! Two doors in:
//! - [`history_list`] — the [`HistoryList`] panel and the rows it draws.
//! - [`init`] — the app-level wiring this crate owns: `history::Focus` moves keyboard
//!   focus to the panel (plan D10: feature crates implement their action handlers).

pub mod history_list;

pub use history_list::{HistoryList, HistoryRow};

use gpui::App;
use pustman_actions::history::Focus;
use workspace::Workspace;

use crate::history_list::PANEL_ID;

/// Wire the panel's app-level action (D10): `history::Focus` puts the keyboard in the
/// history panel from wherever the user is (PRD §8.5).
pub fn init(cx: &mut App) {
    cx.on_action(focus_history_panel);
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
