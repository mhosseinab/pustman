//! One shared workspace id (issue #150): `restore_wiring` saves the window's tabs and
//! layout under the same constant the rest of the app scopes by (`WORKSPACE_ID`), so the
//! picker's per-workspace selection and the restore key can never diverge. Before the
//! fix the value was written twice — `WORKSPACE_ID` here and the module-private
//! `restore_wiring::WORKSPACE_KEY` — and this test could not even name the shared one.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::TestAppContext;
use pustman::{StoreLocation, WORKSPACE_ID};
use storage::LocalStore;
use workspace::Workspace;

/// The save cadence `workspace` debounces and autosaves on — its `SAVE_DEBOUNCE`, which
/// the crate does not re-export.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// One launch over `store` that leaves its layout saved: the app's init order, one
/// window, and a dock size change (the same trigger `wire_restore.rs` uses).
fn launch_saving_its_layout(cx: &mut TestAppContext, store: &Arc<Mutex<LocalStore>>) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(store))));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .update(cx, |dock, cx| dock.set_size_px(320., cx));
        });
    });
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
}

#[gpui::test]
fn test_the_restore_wire_saves_under_the_shared_workspace_id(cx: &mut TestAppContext) {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));

    launch_saving_its_layout(cx, &store);

    let store = store.lock().expect("the lock is free");
    let rows = store.layout().list().expect("the layout reads");
    assert!(
        rows.iter().any(|row| row.workspace_id == WORKSPACE_ID),
        "the layout one window saves is keyed by the shared WORKSPACE_ID {WORKSPACE_ID:?}, got {:?}",
        rows.iter().map(|row| &row.workspace_id).collect::<Vec<_>>()
    );
}
