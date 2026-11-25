//! The `OpenRecent` picker reads the app's one recent list (#332/#333): opening it
//! refreshes the `workspace::recent_workspaces` global the title bar and the welcome pane
//! draw, and the picker lists exactly what that global then holds.

use std::sync::{Arc, Mutex};

use gpui::TestAppContext;
use pustman_actions::workspace::OpenRecent;
use storage::{LocalStore, RecentWorkspace};
use time::OffsetDateTime;
use ui::test_support::{draw_window, init_test};
use workspace::recent_workspaces::recent_workspaces;
use workspace::{Workspace, WorkspaceOpen};

use crate::picker::Picker;
use crate::recent::RecentDelegate;

#[gpui::test]
fn test_open_recent_lists_the_shared_recent_global_after_refreshing_it(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let folder = directory.path().join("billing");
    std::fs::create_dir(&folder).expect("the workspace folder is created");

    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the store migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(init_test);
    cx.update(|cx| {
        crate::init(cx);
        workspace::init(cx);
        WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);

    // Recorded after the global's first read: only a refresh can bring it in.
    store
        .lock()
        .expect("the lock is free")
        .recent_workspaces()
        .insert(&RecentWorkspace {
            path: folder.to_string_lossy().into_owned(),
            opened_at: OffsetDateTime::now_utc(),
        })
        .expect("the recent row is recorded");

    cx.update(|window, cx| window.dispatch_action(Box::new(OpenRecent), cx));
    cx.run_until_parked();
    draw_window(cx);

    let global: Vec<_> = cx.read(|cx| {
        recent_workspaces(cx)
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    });
    assert_eq!(
        global,
        vec![folder.clone()],
        "opening the picker refreshed the shared recent list every other reader draws"
    );
    let listed = cx.read(|cx| {
        workspace
            .read(cx)
            .modal_layer()
            .active_modal::<Picker<RecentDelegate>>()
            .map(|picker| picker.read(cx).delegate().listed_folders())
    });
    assert_eq!(
        listed,
        Some(global),
        "the picker lists exactly the shared recent list"
    );
}
