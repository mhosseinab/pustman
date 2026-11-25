//! The recent workspaces the title bar offers (S48u2): the workspace switcher's list and
//! the app menu's File › Open recent submenu both read [`recent_folders`], so the two can
//! never disagree. Choosing one dispatches [`OpenRecentWorkspace`], which lands in
//! [`WorkspaceOpen::open_folder`] — the one door every open goes through (S41a).
//!
//! `Decision:` the list is an app-wide [`Global`] refreshed when the local store lands
//! and after every workspace open (each open records a row, so the list changes then and
//! only then). The read runs on a background task (AGENTS.md › async) and a folder that
//! no longer exists is left out, the shape `command_palette`'s Open Recent picker reads
//! the same table in. A failed refresh keeps the last list and is logged, not toasted: it
//! runs on its own at launch, nobody asked for it, and the picker (`workspace::OpenRecent`)
//! still reports a failed read to the user who does ask.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{App, Global, Subscription};
use storage::LocalStore;

use crate::workspace_open::{OpenStore, WorkspaceOpen, WorkspaceOpenEvent, with_active_opener};

#[cfg(test)]
#[path = "workspace_switcher_tests.rs"]
mod switcher_tests;

/// Opens one recent workspace folder in the active window: what an entry of the
/// workspace switcher and of File › Open recent runs. `path` is the folder as the recent
/// list recorded it.
//
// `gpui` re-exports the `schemars` its `Action` schema API is built on; the derive's
// generated impl resolves through this alias (the `environment::EditSecret` shape).
use gpui::private::schemars;

#[derive(Clone, PartialEq, Eq, Debug, serde::Deserialize, schemars::JsonSchema, gpui::Action)]
#[action(namespace = workspace)]
pub struct OpenRecentWorkspace {
    /// The workspace folder to open.
    #[serde(default)]
    pub path: String,
}

/// The recent workspace folders that still exist, newest first.
#[derive(Default)]
pub struct RecentWorkspaces(Vec<PathBuf>);

impl Global for RecentWorkspaces {}

/// The recent workspace folders, newest first — empty until the first read lands.
pub fn recent_folders(cx: &App) -> &[PathBuf] {
    cx.try_global::<RecentWorkspaces>()
        .map(|recents| recents.0.as_slice())
        .unwrap_or_default()
}

/// The init guard and the subscriptions that keep the list fresh.
#[derive(Default)]
struct Wiring {
    started: bool,
    _subscriptions: Vec<Subscription>,
}

impl Global for Wiring {}

/// Refresh the list when the store lands and after every open, and answer
/// [`OpenRecentWorkspace`].
pub fn init(cx: &mut App) {
    if cx.default_global::<Wiring>().started {
        return;
    }
    cx.default_global::<Wiring>().started = true;
    cx.on_action(open_recent_workspace);
    let store_landed = cx.observe_global::<OpenStore>(refresh);
    let opened = cx.observe_new::<WorkspaceOpen>(|_opener, _window, cx| {
        // Detached: it lives exactly as long as the opener it watches.
        cx.subscribe_self(|_opener, event: &WorkspaceOpenEvent, cx| {
            if matches!(event, WorkspaceOpenEvent::Opened { .. }) {
                refresh(cx);
            }
        })
        .detach();
    });
    let wiring = cx.default_global::<Wiring>();
    wiring._subscriptions.push(store_landed);
    wiring._subscriptions.push(opened);
    if WorkspaceOpen::local_store(cx).is_some() {
        refresh(cx);
    }
}

/// Re-read the list off the foreground thread and install it.
fn refresh(cx: &mut App) {
    let Some(store) = WorkspaceOpen::local_store(cx) else {
        return;
    };
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move { read_existing_folders(&store) })
            .await;
        match read {
            Some(folders) => cx.update(|cx| cx.set_global(RecentWorkspaces(folders))),
            None => log::error!("the recent workspaces could not be read for the title bar"),
        }
    })
    .detach();
}

/// The recorded folders that still exist, newest first; [`None`] when the list cannot
/// be read. The stat per row runs after the store's lock is released.
fn read_existing_folders(store: &Arc<Mutex<LocalStore>>) -> Option<Vec<PathBuf>> {
    let recorded: Vec<PathBuf> = {
        let store = store.lock().ok()?;
        store
            .recent_workspaces()
            .list()
            .ok()?
            .into_iter()
            .map(|recent| PathBuf::from(recent.path))
            .collect()
    };
    Some(
        recorded
            .into_iter()
            .filter(|folder| folder.is_dir())
            .collect(),
    )
}

/// The [`OpenRecentWorkspace`] answer in the active window.
fn open_recent_workspace(action: &OpenRecentWorkspace, cx: &mut App) {
    let folder = PathBuf::from(&action.path);
    with_active_opener(cx, move |opener, cx| opener.open_folder(folder, cx));
}
