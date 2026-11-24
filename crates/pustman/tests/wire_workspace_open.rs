//! The workspace-open wire: `open_local_store` installs the local store into
//! `WorkspaceOpen`, so in the running app — not only in `workspace`'s tests — opening a
//! workspace records it in the recent list and the next launch reopens it (REQ-WSP-05).
//! One store is shared by both simulated launches: a fresh database per launch would
//! prove nothing, because the second launch would read an empty recent list.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman::{AppStore, StoreLocation};
use workspace::{Workspace, WorkspaceOpen};

/// The workspace folder the user opens: `pustman.json` as a real workspace holds it
/// (PRD §6.1).
fn fixture_workspace(root: &Path) {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Pets workspace",
        "format_version": "1.0",
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the fixture serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the fixture is written");
}

/// The folder the window shows as its workspace, as a path.
fn open_root(opened: &Entity<WorkspaceOpen>, cx: &VisualTestContext) -> Option<PathBuf> {
    cx.read(|cx| opened.read_with(cx, |opened, _| opened.root().map(Path::to_path_buf)))
}

#[gpui::test]
fn wire_workspace_open_records_recent_and_reopens_at_launch(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path();
    fixture_workspace(root);

    // Launch 1: the app's real init entry — a private in-memory store — and its window.
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();

    // The user opens the workspace folder.
    let opened = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    cx.update(|_, cx| {
        opened.update(cx, |opener, cx| {
            opener.open_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();

    // The open is recorded in the recent list through the wire (`AppStore` is the app's
    // one store — the same handle `WorkspaceOpen` was installed with).
    let store = cx
        .read(|cx| cx.try_global::<AppStore>().map(|app| Arc::clone(&app.0)))
        .expect("the app's store landed");
    let recorded = {
        let store = store.lock().expect("the lock is free");
        store
            .recent_workspaces()
            .list()
            .expect("the recent list reads")
    };
    assert_eq!(
        recorded
            .iter()
            .map(|recent| recent.path.clone())
            .collect::<Vec<_>>(),
        vec![root.to_string_lossy().into_owned()],
        "opening a workspace records it in the recent list (REQ-WSP-05)"
    );

    // Launch 2: a fresh init over the same store; its window reopens the last workspace.
    cx.update(|_, cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    let (relaunched, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let reopened = cx
        .read(|cx| WorkspaceOpen::for_window(relaunched.entity_id(), cx))
        .expect("the relaunched window has open-workspace state");
    assert_eq!(
        open_root(&reopened, cx).as_deref(),
        Some(root),
        "the last workspace reopens at launch (REQ-WSP-05)"
    );
}
