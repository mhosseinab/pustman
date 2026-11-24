//! Open and create workspaces (REQ-WSP-05) driven the way a user drives them: the
//! `OpenWorkspace` / `CreateWorkspace` actions and their folder dialog, against a real
//! temp folder and the real local store (`LocalStore::open_in_memory` + `migrate` — no
//! test writes the user's config dir). Every assertion is an outcome the user can see:
//! the new workspace's `pustman.json` and folder layout on disk, the opened workspace in
//! its window, the recent list, and the launch reopen of the last workspace.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{ItemId, Workspace as WorkspaceDocument};
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use storage::LocalStore;
use ui::test_support::init_test;
use workspace_store::FORMAT_VERSION;

use crate::workspace_open::WorkspaceOpen;
use crate::{Workspace, init};

/// The local store the flows record into: in memory and migrated, so no test touches the
/// user's real database.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace window on the app's real init order, with `store` installed the way the
/// app's store wiring installs it: the shape both flows run in.
fn workspace_window<'a>(
    cx: &'a mut TestAppContext,
    store: &Arc<Mutex<LocalStore>>,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    (workspace, cx)
}

/// The workspace folder on disk an open reads: `pustman.json` and one collection
/// (PRD §6.1), written the way a real workspace sits on disk.
fn fixture_workspace(root: &Path) {
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
            "name": "Pets workspace",
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

/// The folder the dialog hands back and what the window shows of it, as a path.
fn open_root(opened: &Entity<WorkspaceOpen>, cx: &VisualTestContext) -> Option<PathBuf> {
    cx.read(|cx| opened.read_with(cx, |opened, _| opened.root().map(Path::to_path_buf)))
}

#[gpui::test]
fn create_workspace_writes_pustman_json(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let chosen = directory.path().join("my-api-workspace");
    std::fs::create_dir(&chosen).expect("the folder the user picks can be created");
    let store = open_store();
    let (_workspace, cx) = workspace_window(cx, &store);

    // The user asks for a new workspace and picks the folder for it.
    cx.dispatch_action(CreateWorkspace);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "CreateWorkspace asks for the workspace folder in a dialog"
    );
    cx.simulate_path_prompt_response(|_| Some(vec![chosen.clone()]));
    cx.run_until_parked();

    let document: WorkspaceDocument = serde_json::from_slice(
        &std::fs::read(chosen.join("pustman.json")).expect("the workspace file is written"),
    )
    .expect("the workspace file is the workspace JSON");
    assert_eq!(
        document.name, "my-api-workspace",
        "the new workspace takes its folder's name"
    );
    assert_eq!(
        document.format_version, FORMAT_VERSION,
        "the new workspace declares the format version this build writes (REQ-WSP-04)"
    );
    assert!(
        chosen.join("collections").is_dir() && chosen.join("environments").is_dir(),
        "the new workspace is created with the PRD §6.1 folder layout"
    );
}

#[gpui::test]
fn open_workspace_loads_snapshot_and_records_recent(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path();
    fixture_workspace(root);
    let store = open_store();
    let (workspace, cx) = workspace_window(cx, &store);

    // The user opens the folder in the dialog.
    cx.dispatch_action(OpenWorkspace);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "OpenWorkspace asks for the workspace folder in a dialog"
    );
    cx.simulate_path_prompt_response(|_| Some(vec![root.to_path_buf()]));
    cx.run_until_parked();

    let opened = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    assert_eq!(
        open_root(&opened, cx).as_deref(),
        Some(root),
        "the chosen folder is this window's workspace (REQ-WSP-05)"
    );
    let names = cx.read(|cx| {
        opened.read_with(cx, |opened, _| {
            let snapshot = opened.snapshot().expect("the workspace snapshot is loaded");
            snapshot
                .items
                .values()
                .map(|item| item.name())
                .collect::<Vec<_>>()
        })
    });
    assert!(
        names.iter().any(|name| name == "Pets"),
        "the snapshot holds the workspace's items: {names:?}"
    );

    // The open is recorded in the recent list ...
    let recorded = {
        let store = store.lock().expect("the lock is free");
        store
            .recent_workspaces()
            .list()
            .expect("the recent list reads")
    };
    assert_eq!(
        recorded.len(),
        1,
        "the opened workspace is recorded in the recent list"
    );
    let first = recorded.first().expect("the recent list has the workspace");
    assert_eq!(
        first.path,
        root.to_string_lossy().into_owned(),
        "the recent entry names the workspace folder"
    );

    // ... and a later launch reopens it (REQ-WSP-05).
    let (relaunched, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let reopened = cx
        .read(|cx| WorkspaceOpen::for_window(relaunched.entity_id(), cx))
        .expect("the relaunched window has open-workspace state");
    assert_eq!(
        open_root(&reopened, cx).as_deref(),
        Some(root),
        "the last workspace reopens at launch"
    );
}
