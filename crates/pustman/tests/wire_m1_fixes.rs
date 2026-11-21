//! M1 fixes (S33o), end to end in the binary's wiring: Ctrl-T's `workspace::NewTab` must
//! open a request tab wherever the request editor is registered (not the Welcome
//! placeholder), and the History panel must list a send recorded *after* launch without
//! being reopened — the panel's live refresh rides the Console feed's per-send
//! notification (`history_list`), which every real send passes on its way out.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use gpui::{AppContext, Entity, TestAppContext};
use history_panel::{HistoryList, HistoryRow};
use http_engine::{FakeTransport, HttpClient};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::request::Send;
use pustman_actions::workspace::{NewRequest, NewTab};
use request_editor::RequestEditor;
use request_editor::send::set_http_client;
use storage::LocalStore;
use ui::test_support::draw_window;
use workspace::Workspace;

/// The URL the test sends to and the History panel must list afterwards.
const URL: &str = "https://example.test/live";

mod m1_fixes_tests {
    use super::*;

    #[gpui::test]
    fn test_new_tab_opens_a_request_editor(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx);

        // The keystroke's action — `assets/keymaps/` pins Ctrl-T to `workspace::NewTab`
        // and the keymap test keeps that binding — must open a request now that the
        // request editor answers `workspace::NewRequest`.
        cx.dispatch_action(NewTab);
        cx.run_until_parked();
        draw_window(cx);

        let active_is_editor = cx.read(|cx| {
            workspace.read_with(cx, |workspace, cx| {
                workspace
                    .center_pane()
                    .read(cx)
                    .active_item()
                    .map(|item| item.to_any_view().downcast::<RequestEditor>().is_ok())
            })
        });
        assert_eq!(
            active_is_editor,
            Some(true),
            "NewTab opens a request editor tab, not the Welcome placeholder (REQ-UI-02/03)"
        );
    }

    #[gpui::test]
    fn test_history_panel_shows_a_send_recorded_after_launch(cx: &mut TestAppContext) {
        // One store shared by the app and the read-back below: the row the wired
        // `http_engine::HistoryRecorder` writes is the one the panel must show.
        let store = shared_store();
        cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
        let transport = FakeTransport::with_response(cx.executor(), 200, b"live".to_vec());
        let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
        cx.update(|cx| set_http_client(client, cx));

        let (workspace, cx) = cx.add_window_view(Workspace::new);
        cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
        cx.update(|window, _cx| window.activate_window());
        // The store opens on a background task; the recorder and the History panel are
        // wired once it lands.
        cx.run_until_parked();
        draw_window(cx);

        let panel = history_panel_of(&workspace, cx).expect("the History panel is registered");
        assert!(
            panel.read_with(cx, |list, _| list.rows().is_empty()),
            "the launch-time panel lists no sends yet"
        );

        // A send made after launch, through the real pipeline: a request tab sending
        // via the wired client — the recorder writes the row, the editor's console feed
        // records the outcome, and the panel's observer reloads on that notification.
        cx.dispatch_action(NewRequest);
        cx.run_until_parked();
        cx.simulate_input(URL);
        cx.dispatch_action(Send);
        cx.run_until_parked();
        draw_window(cx);

        let rows = panel.read_with(cx, |list, _| list.rows().to_vec());
        assert!(
            rows.iter()
                .any(|row| matches!(row, HistoryRow::Entry(entry) if entry.target == URL)),
            "the panel lists the send recorded after launch without being reopened; rows: \
             {:?}",
            rows.iter()
                .map(|row| match row {
                    HistoryRow::Day { label, .. } => label.to_string(),
                    HistoryRow::Entry(entry) => entry.target.clone(),
                })
                .collect::<Vec<_>>()
        );
    }
}

/// The app's one window on the full init order (the `wire_request_editor.rs` shape).
fn open_workspace<'a>(
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut gpui::VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    (workspace, cx)
}

/// The History panel the left dock registered, found by type among the dock's panels.
fn history_panel_of(
    workspace: &Entity<Workspace>,
    cx: &gpui::VisualTestContext,
) -> Option<Entity<HistoryList>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panels()
                .iter()
                .find_map(|panel| panel.to_any_view().downcast::<HistoryList>().ok())
        })
    })
}

/// The in-memory local store both the app and the test read and write through (the
/// `wire_history_recording.rs` shape).
fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}
