//! The recent workspaces (S48u2): the one source every list of them reads — the workspace
//! switcher, the app menu's File › Open recent submenu, the welcome pane's Recent list
//! and the `OpenRecent` picker (#332/#333) — so no two of them can disagree — each entry under the workspace's own name, the one the switcher
//! shows for the open workspace. Choosing one dispatches [`OpenRecentWorkspace`], which lands in
//! [`WorkspaceOpen::open_folder`] — the one door every open goes through (S41a).
//!
//! `Decision:` the list is an app-wide [`Global`] refreshed when the local store lands
//! and after every workspace open (each open records a row, so the list changes then and
//! only then). The read runs on a background task (AGENTS.md › async) and a folder that
//! no longer exists is left out, the shape `command_palette`'s Open Recent picker reads
//! the same table in. A failed refresh keeps the last list, marks it failed — the
//! welcome says so in its Recent list — and is not toasted: it runs on its own at launch,
//! nobody asked for it. The picker (`workspace::OpenRecent`) re-reads through
//! [`reload_recent_workspaces`] and toasts a failed read to the user who does ask.
//!
//! `Decision:` an entry's name is the `name` in its folder's `pustman.json`, read with the
//! same background stat (only that one field, never the whole workspace); a file without
//! a readable name falls back to the folder's name. A newer refresh replaces — cancels —
//! an older one still in flight, so a slow read can never install a stale list.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use gpui::{App, BorrowAppContext, Global, SharedString, Subscription, Task};
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
    /// The workspace folder to open, in [`encode_recent_path`]'s lossless form.
    #[serde(default)]
    pub path: String,
}

impl OpenRecentWorkspace {
    /// The action that opens `folder` — any folder name the platform allows, UTF-8 or
    /// not (#332).
    pub fn for_folder(folder: &Path) -> Self {
        Self {
            path: encode_recent_path(folder),
        }
    }

    /// The folder this action opens, exactly as it was recorded.
    pub fn folder(&self) -> PathBuf {
        decode_recent_path(&self.path)
    }
}

/// What marks a path [`encode_recent_path`] stored as base64 bytes. No absolute path
/// starts with it, and a path that does is encoded too, so decoding is never ambiguous.
const ENCODED_PATH_PREFIX: &str = "base64:";

/// `path` as the text the recent list stores and its menu actions carry (#332): a UTF-8
/// path as itself — every row recorded before stays readable — and any other path (a
/// non-UTF-8 folder name on Linux) as base64 of its bytes, so it reopens exactly instead
/// of as a lossy look-alike that does not exist.
pub fn encode_recent_path(path: &Path) -> String {
    match path.to_str() {
        Some(text) if !text.starts_with(ENCODED_PATH_PREFIX) => text.to_owned(),
        _ => format!("{ENCODED_PATH_PREFIX}{}", STANDARD.encode(path_bytes(path))),
    }
}

/// The path [`encode_recent_path`] stored as `text`. Text that is not an encoded path
/// is a plain path.
pub fn decode_recent_path(text: &str) -> PathBuf {
    text.strip_prefix(ENCODED_PATH_PREFIX)
        .and_then(|encoded| STANDARD.decode(encoded).ok())
        .map(path_from_bytes)
        .unwrap_or_else(|| PathBuf::from(text))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(bytes))
}

/// Off Unix a path's bytes have no safe, stable form, and a non-Unicode path there is
/// carried as its lossy text: the one platform where such names are common is Linux.
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from(String::from_utf8_lossy(&bytes).into_owned()))
}

/// One recent workspace: its folder and the name its `pustman.json` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentEntry {
    /// The workspace folder.
    pub path: PathBuf,
    /// The workspace's own name, or its folder's name when the file has none.
    pub name: String,
}

/// The recent workspaces that still exist, newest first, and whether the last read of
/// them failed.
#[derive(Default)]
pub struct RecentWorkspaces {
    pub(crate) entries: Vec<RecentEntry>,
    pub(crate) failed: bool,
}

impl Global for RecentWorkspaces {}

/// The recent workspaces, newest first — empty until the first read lands.
pub fn recent_workspaces(cx: &App) -> &[RecentEntry] {
    cx.try_global::<RecentWorkspaces>()
        .map(|recents| recents.entries.as_slice())
        .unwrap_or_default()
}

/// Whether the last read of the recent list failed; the list then holds what the read
/// before it found.
pub fn recent_workspaces_failed(cx: &App) -> bool {
    cx.try_global::<RecentWorkspaces>()
        .is_some_and(|recents| recents.failed)
}

/// Why [`reload_recent_workspaces`] could not refresh the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentReadError {
    /// The app has not installed its local store yet ([`WorkspaceOpen::local_store`]).
    NotConnected,
    /// The store could not be read; the list keeps what it held.
    Unreadable,
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
    refreshing: Option<Task<Result<(), RecentReadError>>>,
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

/// Re-read the list off the foreground thread and install it, replacing — cancelling —
/// a refresh still in flight.
fn refresh(cx: &mut App) {
    let task = reload_recent_workspaces(cx);
    cx.default_global::<Wiring>().refreshing = Some(task);
}

/// Re-read the recent list off the foreground thread and install it in the one
/// [`RecentWorkspaces`] global every reader draws from; the task resolves once the
/// global holds the result. A failed read keeps the last list and marks it failed.
pub fn reload_recent_workspaces(cx: &mut App) -> Task<Result<(), RecentReadError>> {
    let Some(store) = WorkspaceOpen::local_store(cx) else {
        return Task::ready(Err(RecentReadError::NotConnected));
    };
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move { read_existing_workspaces(&store) })
            .await;
        cx.update(|cx| {
            cx.update_default_global::<RecentWorkspaces, _>(|recents, _cx| match read {
                Some(entries) => {
                    recents.entries = entries;
                    recents.failed = false;
                    Ok(())
                }
                None => {
                    log::error!("the recent workspaces could not be read");
                    recents.failed = true;
                    Err(RecentReadError::Unreadable)
                }
            })
        })
    })
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
            .map(|recent| decode_recent_path(&recent.path))
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
    let folder = action.folder();
    with_active_opener(cx, move |opener, cx| opener.open_folder(folder, cx));
}

#[cfg(test)]
#[path = "recent_single_source_tests.rs"]
mod single_source_tests;

#[cfg(test)]
#[path = "recent_non_utf8_tests.rs"]
mod non_utf8_tests;
