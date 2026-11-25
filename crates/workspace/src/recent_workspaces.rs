//! The recent workspaces the title bar offers (S48u2): the workspace switcher's list and
//! the app menu's File › Open recent submenu both read [`recent_workspaces`], so the two
//! can never disagree — each entry under the workspace's own name, the one the switcher
//! shows for the open workspace. Choosing one dispatches [`OpenRecentWorkspace`], which lands in
//! [`WorkspaceOpen::open_folder`] — the one door every open goes through (S41a).
//!
//! `Decision:` the list is an app-wide [`Global`] refreshed when the local store lands
//! and after every workspace open (each open records a row, so the list changes then and
//! only then). The read runs on a background task (AGENTS.md › async) and a folder that
//! no longer exists is left out, the shape `command_palette`'s Open Recent picker reads
//! the same table in. A failed refresh keeps the last list and is logged, not toasted: it
//! runs on its own at launch, nobody asked for it, and the picker (`workspace::OpenRecent`)
//! still reports a failed read to the user who does ask.
//!
//! `Decision:` an entry's name is the `name` in its folder's `pustman.json`, read with the
//! same background stat (only that one field, never the whole workspace); a file without
//! a readable name falls back to the folder's name. A newer refresh replaces — cancels —
//! an older one still in flight, so a slow read can never install a stale list.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{App, Global, SharedString, Subscription, Task};
use storage::LocalStore;

use crate::workspace_open::{
    OpenStore, WORKSPACE_FILE, WorkspaceOpen, WorkspaceOpenEvent, with_active_opener,
};

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

/// One recent workspace: its folder and the name its `pustman.json` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    /// The workspace folder.
    pub path: PathBuf,
    /// The workspace's own name, or its folder's name when the file has none.
    pub name: String,
}

/// The recent workspaces that still exist, newest first.
#[derive(Default)]
pub struct RecentWorkspaces(Vec<RecentEntry>);

impl Global for RecentWorkspaces {}

/// The recent workspaces, newest first — empty until the first read lands.
pub fn recent_workspaces(cx: &App) -> &[RecentEntry] {
    cx.try_global::<RecentWorkspaces>()
        .map(|recents| recents.0.as_slice())
        .unwrap_or_default()
}

/// Where `entries[index]` lives, when another recent workspace has the same name: the
/// one detail that tells the two apart. [`None`] for a unique name.
pub fn disambiguation(entries: &[RecentEntry], index: usize) -> Option<SharedString> {
    let entry = entries.get(index)?;
    let shared = entries
        .iter()
        .filter(|other| other.name == entry.name)
        .count()
        > 1;
    shared.then(|| SharedString::from(entry.path.to_string_lossy().into_owned()))
}

/// The init guard, the subscriptions that keep the list fresh, and the refresh in
/// flight (a newer one replaces — cancels — it).
#[derive(Default)]
struct Wiring {
    started: bool,
    refreshing: Option<Task<()>>,
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
    let task = cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move { read_existing_workspaces(&store) })
            .await;
        match read {
            Some(entries) => cx.update(|cx| cx.set_global(RecentWorkspaces(entries))),
            None => log::error!("the recent workspaces could not be read for the title bar"),
        }
    });
    cx.default_global::<Wiring>().refreshing = Some(task);
}

/// The recorded workspaces whose folders still exist, newest first, each named from its
/// `pustman.json`; [`None`] when the list cannot be read. The stat and the name read per
/// row run after the store's lock is released.
fn read_existing_workspaces(store: &Arc<Mutex<LocalStore>>) -> Option<Vec<RecentEntry>> {
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
            .map(|path| {
                let name = workspace_name(&path);
                RecentEntry { path, name }
            })
            .collect(),
    )
}

/// Just the `name` of a `pustman.json`: the one field the list needs.
#[derive(serde::Deserialize)]
struct NamedWorkspace {
    name: String,
}

/// The name `folder`'s `pustman.json` gives the workspace, or the folder's own name when
/// the file has none (unreadable, unparsable or blank). An unreadable file is not this
/// list's to report: opening the workspace reports it.
fn workspace_name(folder: &Path) -> String {
    std::fs::read(folder.join(WORKSPACE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<NamedWorkspace>(&bytes).ok())
        .map(|workspace| workspace.name)
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| folder.to_string_lossy().into_owned())
}

/// The [`OpenRecentWorkspace`] answer in the active window.
fn open_recent_workspace(action: &OpenRecentWorkspace, cx: &mut App) {
    let folder = PathBuf::from(&action.path);
    with_active_opener(cx, move |opener, cx| opener.open_folder(folder, cx));
}
