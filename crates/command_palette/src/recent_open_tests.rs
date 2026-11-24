//! The recent picker's open path in the app's own wiring: the store comes from
//! [`WorkspaceOpen::install_local_store`] (what `pustman::open_local_store` calls — no
//! hand-installed inputs), confirming a row opens its folder through `workspace_open`,
//! and the two states that are not a list reach the user as toasts.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman_actions::workspace::OpenRecent;
use storage::{LocalStore, RecentWorkspace};
use time::{Duration, OffsetDateTime};
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{Workspace, WorkspaceOpen};

use crate::picker::Picker;
use crate::recent::{EMPTY_MESSAGE, NOT_WIRED_MESSAGE, RecentDelegate};

/// The app's local store as `pustman::open_local_store` builds it: in-memory and migrated.
fn opened_store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the store migrates");
    store
}

/// The workspace folder at `root`: `pustman.json` as a real workspace holds it (PRD
/// §6.1), so `workspace_open` can open it.
fn fixture_workspace(root: &Path) {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Recent workspace",
        "format_version": "1.0",
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the fixture serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the fixture is written");
}

/// One window on the app's init order, with `store` installed where the app's
/// store-landing code installs it ([`WorkspaceOpen::install_local_store`]) — or none at
/// all, the state before the store lands.
fn recent_window(
    cx: &mut TestAppContext,
    store: Option<Arc<Mutex<LocalStore>>>,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        crate::init(cx);
        workspace::init(cx);
        if let Some(store) = store {
            WorkspaceOpen::install_local_store(store, cx);
        }
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// Dispatch `workspace::OpenRecent` the way the command palette does — the keymap does
/// not bind it yet (see the build report's Decision).
fn dispatch_open_recent(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(OpenRecent), cx));
}

/// The folder the window has open, as a path — what a confirmed row must leave behind.
fn open_root(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<PathBuf> {
    let opened = cx.read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))?;
    cx.read(|cx| opened.read_with(cx, |opened, _| opened.root().map(Path::to_path_buf)))
}

/// Whether the recent picker is the active modal.
fn picker_is_open(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<Picker<RecentDelegate>>()
                .is_some()
        })
    })
}

#[gpui::test]
fn confirming_a_recent_row_opens_its_folder(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let older = directory.path().join("older-workspace");
    let newer = directory.path().join("newer-workspace");
    for folder in [&older, &newer] {
        std::fs::create_dir(folder).expect("the workspace folder is created");
        fixture_workspace(folder);
    }

    let store = Arc::new(Mutex::new(opened_store()));
    let (workspace, cx) = recent_window(cx, Some(Arc::clone(&store)));
    // The rows land after the window exists, so the launch reopen finds nothing and
    // cannot pre-empt what the confirm below proves.
    let now = OffsetDateTime::now_utc();
    for (folder, opened_at) in [(&older, now - Duration::hours(1)), (&newer, now)] {
        store
            .lock()
            .expect("the lock is free")
            .recent_workspaces()
            .insert(&RecentWorkspace {
                path: folder.to_string_lossy().into_owned(),
                opened_at,
            })
            .expect("the recent row is recorded");
    }

    dispatch_open_recent(cx);
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        picker_is_open(&workspace, cx),
        "the recent picker opens over the recorded folders"
    );
    assert_eq!(open_root(&workspace, cx), None, "no workspace is open yet");

    // The second row, not the top one: this proves the *confirmed* row's folder opens.
    press_key(cx, "down");
    press_key(cx, "enter");
    cx.run_until_parked();

    assert_eq!(
        open_root(&workspace, cx),
        Some(older),
        "confirming a row opens its folder through workspace_open"
    );
}

#[gpui::test]
fn open_recent_without_a_store_shows_the_not_connected_toast(cx: &mut TestAppContext) {
    // No store is installed: the state before `pustman::open_local_store`'s store lands.
    let (workspace, cx) = recent_window(cx, None);

    dispatch_open_recent(cx);
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(toast_message_selector(NOT_WIRED_MESSAGE).leak())
            .is_some(),
        "the user is told why no list can be shown"
    );
    assert!(
        !picker_is_open(&workspace, cx),
        "a picker that cannot list anything does not open"
    );
}

#[gpui::test]
fn an_empty_recent_list_shows_the_empty_toast(cx: &mut TestAppContext) {
    // A store that records nothing — the state on a first run.
    let (workspace, cx) = recent_window(cx, Some(Arc::new(Mutex::new(opened_store()))));

    dispatch_open_recent(cx);
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.debug_bounds(toast_message_selector(EMPTY_MESSAGE).leak())
            .is_some(),
        "nothing to choose is reported, not listed"
    );
    assert!(
        !picker_is_open(&workspace, cx),
        "an empty list opens no picker"
    );
}
