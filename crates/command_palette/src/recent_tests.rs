//! The recent list is what the user reads it as: newest first (REQ-WSP-05), and never
//! a folder that no longer exists — `storage` neither canonicalizes nor prunes its
//! `recent_workspaces` rows (issues #304/#305), so the picker is where a recorded but
//! deleted folder has to stop being offered.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman_actions::workspace::OpenRecent;
use storage::{LocalStore, RecentWorkspace};
use time::{Duration, OffsetDateTime};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use crate::picker::Picker;
use crate::recent::{RecentDelegate, RecentInputs};

/// The folders behind the picker's rows, top first, as the user reads them. The picker
/// is open.
fn listed_folders(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    cx.read(|cx| {
        let picker = workspace
            .read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<Picker<RecentDelegate>>()
            })
            .expect("the recent picker is open");
        picker.read_with(cx, |picker, _| picker.delegate().listed_folders())
    })
}

#[gpui::test]
fn recent_list_most_recent_first(cx: &mut TestAppContext) {
    // Two recorded folders that exist and one whose folder is gone — its row outlives it,
    // exactly as a deleted workspace folder leaves its row behind (issues #304/#305).
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let older = directory.path().join("older-workspace");
    let gone = directory.path().join("gone-workspace");
    let newer = directory.path().join("newer-workspace");
    std::fs::create_dir(&older).expect("the older folder is created");
    std::fs::create_dir(&newer).expect("the newer folder is created");

    cx.update(init_test);
    cx.update(|cx| {
        crate::init(cx);
        workspace::init(cx);
        let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
        store.migrate().expect("the store migrates");
        let now = OffsetDateTime::now_utc();
        for (folder, opened_at) in [
            (&older, now - Duration::hours(2)),
            (&gone, now - Duration::hours(1)),
            (&newer, now),
        ] {
            store
                .recent_workspaces()
                .insert(&RecentWorkspace {
                    path: folder.to_string_lossy().into_owned(),
                    opened_at,
                })
                .expect("the recent row is recorded");
        }
        crate::recent::install_inputs(
            cx,
            RecentInputs {
                store: Arc::new(Mutex::new(store)),
            },
        );
    });

    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    cx.update(|window, cx| window.dispatch_action(Box::new(OpenRecent), cx));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        listed_folders(&workspace, cx),
        vec![newer, older],
        "the recent list is newest first (REQ-WSP-05) and never offers a folder that is gone"
    );
}
