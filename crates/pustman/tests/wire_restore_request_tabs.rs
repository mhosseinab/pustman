//! The request-editor kind on the restore wire (issue #103): a request tab that was open
//! when a launch closed comes back as a request tab holding its draft, and the
//! "missing items" toast a kind-less restore shows never appears (PRD REQ-UI-02). The
//! production wire does the registering here — the test only runs the app's init order,
//! so a kind nobody registers in production fails this test.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};
use pustman::StoreLocation;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use storage::LocalStore;
use workspace::{SAVE_DEBOUNCE, Workspace};

/// The URL the first launch types into the draft — what the restored tab must hold.
const EDITED_URL: &str = "https://api.example.test/v1?draft=edited";

/// The one store both launches run over: in memory and migrated, shared by injection.
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Let the debounced save — and one autosave tick — write through.
fn settle(cx: &mut VisualTestContext) {
    cx.executor()
        .advance_clock(SAVE_DEBOUNCE + Duration::from_millis(100));
    cx.run_until_parked();
}

#[gpui::test]
fn wire_restore_request_tabs_rebuild_the_editor_from_its_draft(cx: &mut TestAppContext) {
    let store = shared_store();

    // Launch 1: the app's init order over the shared store, then its one window.
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();

    // One request tab, opened the production way, then edited after it opened — the
    // draft the save has to carry.
    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    let (editor, saved_id) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let editor = workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
                .expect("NewRequest opened a request editor tab");
            let saved_id = editor.read(cx).draft().id;
            (editor, saved_id)
        })
    });
    cx.update(|_, cx| {
        let url_bar = editor.read(cx).url_bar().clone();
        url_bar.update(cx, |bar, cx| bar.set_url(EDITED_URL, cx));
    });
    cx.run_until_parked();
    settle(cx);

    // Launch 2: a fresh init over the same store; its window must come back whole.
    cx.update(|_window, cx| {
        pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store)))
    });
    let (relaunched, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();

    let (tab_count, rebuilt, no_missing_toast) = cx.read(|cx| {
        relaunched.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.item_ids().len(),
                pane.active_item()
                    .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok()),
                workspace.toasts().read(cx).is_empty(),
            )
        })
    });
    assert_eq!(
        tab_count, 1,
        "the request tab comes back after the relaunch (REQ-UI-02)"
    );
    assert!(
        no_missing_toast,
        "the registered request-editor kind claims the tab, so no missing-items toast \
         shows (issue #103)"
    );
    let rebuilt = rebuilt.expect("the restored tab is a request editor tab, not a missing item");
    let draft = cx.read(|cx| rebuilt.read(cx).draft().clone());
    assert_eq!(
        draft.id, saved_id,
        "the tab rebuilds under its own request id (REQ-UI-02)"
    );
    assert_eq!(
        draft.url, EDITED_URL,
        "the draft the autosave saved comes back with the tab (REQ-NFR-05)"
    );
}
