//! The workspace switcher after the menus (S48u2): it names the open workspace ("No
//! workspace" before one opens), and its popover lists the recent workspaces — newest
//! first, gone folders left out — plus New workspace… and Open workspace…. Choosing a
//! recent one opens it in this window. The store is the real one in memory and the
//! folders are real temp folders.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::ItemId;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use storage::{LocalStore, RecentWorkspace};
use time::{Duration, OffsetDateTime};
use ui::components::context_menu::MENU_SELECTOR;
use ui::test_support::{draw_window, init_test};
use workspace_store::FORMAT_VERSION;

use crate::recent_workspaces::recent_folders;
use crate::title_bar::{NO_WORKSPACE_LABEL, SWITCHER_SELECTOR, workspace_display_name};
use crate::workspace_open::WorkspaceOpen;
use crate::{Workspace, init};

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace folder on disk named `name`, whose `pustman.json` calls it `title`.
fn fixture_workspace(root: &Path, title: &str) {
    std::fs::create_dir_all(root).expect("the fixture folder is created");
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": title,
        "format_version": FORMAT_VERSION,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_vec_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the fixture workspace file is written");
}

/// Record `folders` in the recent list, the first one newest.
fn record_recents(store: &Arc<Mutex<LocalStore>>, folders: &[&Path]) {
    let store = store.lock().expect("the lock is free");
    let now = OffsetDateTime::now_utc();
    for (age, folder) in folders.iter().enumerate() {
        let age = i64::try_from(age).expect("a handful of folders");
        store
            .recent_workspaces()
            .insert(&RecentWorkspace {
                path: folder.to_string_lossy().into_owned(),
                opened_at: now - Duration::minutes(age),
            })
            .expect("the recent row is recorded");
    }
}

fn workspace_window<'a>(
    cx: &'a mut TestAppContext,
    store: Option<&Arc<Mutex<LocalStore>>>,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    if let Some(store) = store {
        cx.update(|_window, cx| WorkspaceOpen::install_local_store(Arc::clone(store), cx));
    }
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn open_root(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    cx.update(|_window, cx| {
        WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .and_then(|opener| opener.read(cx).root().map(Path::to_path_buf))
    })
}

#[gpui::test]
fn test_the_switcher_says_no_workspace_before_one_opens(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx, None);
    let name = cx.update(|_window, cx| workspace_display_name(workspace.entity_id(), cx));
    assert_eq!(name.as_ref(), NO_WORKSPACE_LABEL);
    assert!(cx.debug_bounds(SWITCHER_SELECTOR).is_some());
}

#[gpui::test]
fn test_the_switcher_lists_recent_workspaces_and_opens_the_one_chosen(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let pets = directory.path().join("pets");
    let orders = directory.path().join("orders");
    let gone = directory.path().join("gone");
    fixture_workspace(&pets, "Pets API");
    fixture_workspace(&orders, "Orders API");
    let store = open_store();
    record_recents(&store, &[&pets, &orders, &gone]);

    let (workspace, cx) = workspace_window(cx, Some(&store));
    // The launch reopen brought back the newest one, and the switcher names it.
    assert_eq!(open_root(&workspace, cx).as_deref(), Some(pets.as_path()));
    let name = cx.update(|_window, cx| workspace_display_name(workspace.entity_id(), cx));
    assert_eq!(
        name.as_ref(),
        "Pets API",
        "the switcher names the open workspace"
    );
    assert_eq!(
        cx.update(|_window, cx| recent_folders(cx).to_vec()),
        vec![pets.clone(), orders.clone()],
        "the recent list is newest first, without the folder that is gone"
    );

    click(cx, SWITCHER_SELECTOR);
    assert!(
        cx.debug_bounds(MENU_SELECTOR).is_some(),
        "the switcher opens"
    );
    for entry in ["pets", "orders", "New workspace…", "Open workspace…"] {
        assert!(
            cx.debug_bounds(entry).is_some(),
            "the switcher lists {entry}"
        );
    }
    assert!(
        cx.debug_bounds("gone").is_none(),
        "a gone folder is not offered"
    );

    click(cx, "orders");

    assert_eq!(
        open_root(&workspace, cx).as_deref(),
        Some(orders.as_path()),
        "choosing a recent workspace opens it in this window"
    );
    assert!(
        cx.debug_bounds(MENU_SELECTOR).is_none(),
        "and closes the switcher"
    );
    let name = cx.update(|_window, cx| workspace_display_name(workspace.entity_id(), cx));
    assert_eq!(name.as_ref(), "Orders API");
}
