//! `OpenRecent` (PRD REQ-WSP-05): a picker over the recent workspaces, newest first —
//! Enter opens the chosen folder in this window.
//!
//! The rows are `storage`'s `recent_workspaces` — the rows [`WorkspaceOpen`] records on
//! every open and the launch reopen reads. `Decision:` they are read on a background
//! task and offered newest first ([`storage`] orders them by `opened_at` descending): a
//! database read is IO and never belongs on the foreground thread (AGENTS.md › async).
//!
//! `Decision:` the list is read from the app's one local store through
//! [`WorkspaceOpen::local_store`] — the store [`WorkspaceOpen`] already holds (S41a's
//! `install_local_store`), so the shipped app lists recents with no further wiring. A
//! picker opened before the store lands says so in a toast (AGENTS.md › errors reach the
//! user) instead of silently listing nothing.
//!
//! `Decision:` a recent folder that no longer exists is left out of the list rather than
//! offered and then refused (issues #304/#305: the store neither canonicalizes its paths
//! nor prunes rows whose folder is gone). Whether a folder still exists is a stat per
//! row, answered on the same background read — never on the foreground thread. A folder
//! deleted between the listing and Enter is still reported by
//! [`WorkspaceOpen::open_folder`]'s own failure toast.
//!
//! `Decision:` confirming opens through [`WorkspaceOpen::open_folder`] — the one door the
//! folder dialog, the launch reopen and this picker all land in (S41a) — instead of
//! re-reading the folder here, so one open path records the recent row and swaps the
//! window's workspace.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{AnyElement, App, Context, DismissEvent, ElementId, Entity, SharedString, Window};
use pustman_actions::workspace::OpenRecent;
use storage::LocalStore;
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen};

use crate::fuzzy;
use crate::picker::{Picker, PickerDelegate};

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;

/// The one message the opener shows when the app has not installed a local store yet
/// ([`WorkspaceOpen::local_store`] is [`None`]). `pub(crate)`: the tests pin it.
pub(crate) const NOT_WIRED_MESSAGE: &str =
    "The recent workspaces cannot be listed — the local store is not connected";

/// The one message a failed read of the recent list shows. `storage`'s error text can
/// quote the rows it failed on and never reaches the user (the rule
/// `workspace::workspace_open` follows for the same table).
pub(crate) const READ_FAILURE_MESSAGE: &str = "The recent workspaces could not be read";

/// What the opener says instead of an empty picker: nothing to choose is not a list the
/// user should search. `pub(crate)`: the tests pin it.
pub(crate) const EMPTY_MESSAGE: &str = "There are no recent workspaces to open";

/// The one message a window with no open-workspace state shows (every window gets one
/// from `workspace::init`; this is the report if one somehow runs without it).
pub(crate) const NO_WORKSPACE_MESSAGE: &str = "This window cannot open a workspace folder";

/// What [`install_inputs`] installs: the app's local store. `Decision:` kept as the test
/// seam — `recent_tests.rs` (read-only) installs through this — and it forwards to
/// [`WorkspaceOpen::install_local_store`], the door the app's store-landing code uses, so
/// a test and the shipped app run the same wiring. The opener itself only ever reads
/// [`WorkspaceOpen::local_store`].
pub struct RecentInputs {
    /// The app's local store the recent list is read from.
    pub store: Arc<Mutex<LocalStore>>,
}

/// Install the store exactly where the app installs it
/// ([`WorkspaceOpen::install_local_store`]); the next `workspace::OpenRecent` lists
/// through [`WorkspaceOpen::local_store`] either way.
pub fn install_inputs(cx: &mut App, inputs: RecentInputs) {
    WorkspaceOpen::install_local_store(inputs.store, cx);
}

/// What one background read of the recent list produced.
enum Listing {
    /// The recorded folders that still exist, newest first.
    Folders(Vec<PathBuf>),
    /// The list could not be read (see [`READ_FAILURE_MESSAGE`]).
    Failed,
    /// No store is installed yet ([`WorkspaceOpen::local_store`] is [`None`]; see
    /// [`NOT_WIRED_MESSAGE`]).
    NotWired,
}

/// The delegate behind the picker: the recent workspaces newest first, fuzzy-filtered by
/// [`fuzzy`] against the query, opened through [`WorkspaceOpen::open_folder`] on confirm.
pub struct RecentDelegate {
    opener: Entity<WorkspaceOpen>,
    /// The folders, newest first — the order the rows draw in before a query re-ranks.
    folders: Vec<PathBuf>,
    /// Indices into `folders`, in rank order — what the list draws.
    matches: Vec<usize>,
    selected: usize,
}

impl RecentDelegate {
    /// A picker over `folders` (newest first), opening the confirmed one in `opener`'s
    /// window.
    pub fn new(opener: Entity<WorkspaceOpen>, folders: Vec<PathBuf>) -> Self {
        let matches = (0..folders.len()).collect();
        Self {
            opener,
            folders,
            matches,
            selected: 0,
        }
    }

    /// The folders behind the rows the list draws, top first.
    pub fn listed_folders(&self) -> Vec<PathBuf> {
        self.matches
            .iter()
            .filter_map(|folder_index| self.folders.get(*folder_index).cloned())
            .collect()
    }

    /// The folder behind match `index`, if there is one.
    fn folder_at(&self, index: usize) -> Option<&Path> {
        self.matches
            .get(index)
            .and_then(|folder_index| self.folders.get(*folder_index))
            .map(PathBuf::as_path)
    }
}

impl PickerDelegate for RecentDelegate {
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
        let displays: Vec<String> = self
            .folders
            .iter()
            .map(|folder| folder.to_string_lossy().into_owned())
            .collect();
        self.matches = fuzzy::rank(query, displays.iter().map(String::as_str))
            .into_iter()
            .map(|string_match| string_match.candidate_index)
            .collect();
        cx.notify();
    }

    fn confirm(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(folder) = self.folder_at(self.selected) else {
            return;
        };
        let folder = folder.to_path_buf();
        // Dismiss first: the modal layer restores focus as it goes, and the open lands in
        // the workspace this picker is leaving. The open runs on its own background task
        // and reports its own failures ([`WorkspaceOpen::open_folder`]).
        cx.emit(DismissEvent);
        self.opener
            .update(cx, |opener, cx| opener.open_folder(folder, cx));
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        index: usize,
        _selected: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let Some(folder) = self.folder_at(index) else {
            return div().into_any_element();
        };
        let display = folder.to_string_lossy().into_owned();
        h_flex()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .id(format!("recent-path-{display}"))
                    .min_w_0()
                    .flex_1()
                    .tooltip(Tooltip::text(display.clone()).builder())
                    .child(Label::new(display).truncate()),
            )
            .into_any_element()
    }

    fn match_key(&self, index: usize) -> ElementId {
        // The folder is the row's identity — never its rank, which the query changes.
        self.folder_at(index)
            .map(|folder| ElementId::from(format!("recent-folder-{}", folder.to_string_lossy())))
            .unwrap_or_else(|| ElementId::from("recent-folder"))
    }

    fn placeholder_text(&self) -> SharedString {
        SharedString::new_static("Search recent workspaces")
    }
}

/// Wire the picker to `workspace::OpenRecent` (PRD REQ-WSP-05): list the recent
/// workspaces over the active workspace, newest first.
pub fn init(cx: &mut App) {
    cx.on_action(open_recent);
}

/// Open the picker in the active window. The list is read on a background task first
/// (AGENTS.md › async), so this only starts the read; the picker opens from its
/// continuation, and every outcome that is not a list reaches the user as a toast.
fn open_recent(_: &OpenRecent, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    let store = WorkspaceOpen::local_store(cx);
    cx.spawn(async move |cx| {
        let listing = match store {
            Some(store) => {
                cx.background_executor()
                    .spawn(async move { recent_folders(&store) })
                    .await
            }
            None => Listing::NotWired,
        };
        let shown = cx.update(|cx| {
            window_handle.update(cx, |root, window, cx| {
                let Ok(workspace_handle) = root.downcast::<Workspace>() else {
                    return;
                };
                let opener = WorkspaceOpen::for_window(workspace_handle.entity_id(), cx);
                workspace_handle.update(cx, |workspace, cx| {
                    let Some(opener) = opener else {
                        workspace.show_toast(NO_WORKSPACE_MESSAGE, Severity::Error, None, cx);
                        return;
                    };
                    match listing {
                        Listing::Folders(folders) if !folders.is_empty() => {
                            workspace.toggle_modal(window, cx, |window, cx| {
                                Picker::new(RecentDelegate::new(opener, folders), window, cx)
                            });
                        }
                        Listing::Folders(_) => {
                            workspace.show_toast(EMPTY_MESSAGE, Severity::Info, None, cx);
                        }
                        Listing::Failed => {
                            workspace.show_toast(READ_FAILURE_MESSAGE, Severity::Error, None, cx);
                        }
                        Listing::NotWired => {
                            workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        }
                    }
                });
            })
        });
        if shown.is_err() {
            // Handled: the window closed between the action and this callback; there is
            // no workspace left to list recent workspaces for and nobody left to tell.
        }
    })
    .detach();
}

/// The recent workspaces to offer, off the foreground thread (AGENTS.md › async): the
/// rows `storage` records, newest first, minus the folders that no longer exist (the
/// module docs on issues #304/#305). The rows are read under the store's lock and the
/// folders statted after it is released: a stat per row is IO the store's other users
/// must not wait on (the review's LOW).
fn recent_folders(store: &Mutex<LocalStore>) -> Listing {
    let recorded = {
        let Ok(store) = store.lock() else {
            // A poisoned lock means another writer panicked mid-row; the list is unreadable.
            return Listing::Failed;
        };
        let Ok(recents) = store.recent_workspaces().list() else {
            return Listing::Failed;
        };
        recents
            .into_iter()
            .map(|recent| PathBuf::from(recent.path))
            .collect::<Vec<_>>()
    };
    Listing::Folders(
        recorded
            .into_iter()
            .filter(|folder| folder.is_dir())
            .collect(),
    )
}
