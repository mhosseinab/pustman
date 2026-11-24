//! The two integrity guarantees around an open or create in flight (the review's MEDIUM
//! and LOW): a store that lands mid-flow never drops the user's own open, and a create
//! never replaces a `pustman.json` another writer claimed first. Both are driven through
//! [`WorkspaceOpen`] against real temp folders — the first with no pump between the
//! user's open and the store landing, which is exactly the window the bug lived in.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext};
use storage::{LocalStore, RecentWorkspace};
use time::OffsetDateTime;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace_store::FORMAT_VERSION;

use crate::workspace_open::{CreateWorkspaceError, WorkspaceOpen};
use crate::{Workspace, init};

/// The local store the flows record into: in memory and migrated.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace window on the app's real init order, with no local store installed — the
/// window as it is before the app's store finishes opening.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    cx.add_window_view(Workspace::new)
}

/// The workspace folder on disk an open reads: `pustman.json` and one collection
/// (PRD §6.1).
fn fixture_workspace(root: &Path, name: &str) {
    let write = |path: PathBuf, value: &serde_json::Value| {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the fixture's directories are created");
        }
        let bytes = serde_json::to_string_pretty(value).expect("the fixture serializes");
        std::fs::write(path, bytes).expect("the fixture file is written");
    };
    write(
        root.join("pustman.json"),
        &serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": name,
            "format_version": FORMAT_VERSION,
        }),
    );
    write(
        root.join("collections/pets/collection.json"),
        &serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "Pets",
        }),
    );
}

/// The folder the window shows as its workspace, as a path.
fn open_root(opened: &Entity<WorkspaceOpen>, cx: &VisualTestContext) -> Option<PathBuf> {
    cx.read(|cx| opened.read_with(cx, |opened, _| opened.root().map(Path::to_path_buf)))
}

#[gpui::test]
fn store_landing_never_drops_an_open_in_flight(cx: &mut TestAppContext) {
    let chosen = tempfile::tempdir().expect("the temp dir must be created");
    fixture_workspace(chosen.path(), "Chosen workspace");
    let last = tempfile::tempdir().expect("the temp dir must be created");
    fixture_workspace(last.path(), "Last workspace");
    let store = open_store();
    // The recent list a previous run left behind: the launch reopen would choose this
    // folder, and must lose to the open the user just asked for.
    store
        .lock()
        .expect("the lock is free")
        .recent_workspaces()
        .insert(&RecentWorkspace {
            path: last.path().to_string_lossy().into_owned(),
            opened_at: OffsetDateTime::now_utc(),
        })
        .expect("the recent entry inserts");

    let (workspace, cx) = workspace_window(cx);
    let opened = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    // The user's own open, in flight — deliberately not pumped yet.
    cx.update(|_, cx| {
        opened.update(cx, |opener, cx| {
            opener.open_folder(chosen.path().to_path_buf(), cx);
        });
    });
    // The app's store lands now, mid-open.
    cx.update(|_, cx| WorkspaceOpen::install_local_store(Arc::clone(&store), cx));
    cx.run_until_parked();

    assert_eq!(
        open_root(&opened, cx).as_deref(),
        Some(chosen.path()),
        "the workspace the user opened wins over the launch reopen (REQ-WSP-05)"
    );
}

// The other writer's file is a dangling symlink, which only Unix can create unprivileged.
#[cfg(unix)]
#[gpui::test]
fn create_never_overwrites_a_pustman_json_another_writer_left(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path();
    // The name another writer claimed a moment ago: a link to nothing, so nothing can
    // read it — exactly the file the check-then-write this replaces could not see.
    let claimed = root.join("pustman.json");
    std::os::unix::fs::symlink("written-by-someone-else", &claimed)
        .expect("the claimed name is taken");

    let store = open_store();
    let (workspace, cx) = workspace_window(cx);
    cx.update(|_, cx| WorkspaceOpen::install_local_store(Arc::clone(&store), cx));
    let opened = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    cx.update(|_, cx| {
        opened.update(cx, |opener, cx| {
            opener.create_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        std::fs::symlink_metadata(&claimed)
            .expect("the claimed name is still there")
            .file_type()
            .is_symlink(),
        "the create never replaces a pustman.json it did not write"
    );
    let message = CreateWorkspaceError::AlreadyAWorkspace(root.to_path_buf()).to_string();
    let selector: &'static str = toast_message_selector(&message).leak();
    assert!(
        cx.debug_bounds(selector).is_some(),
        "the refusal reaches the user in a toast"
    );
}
