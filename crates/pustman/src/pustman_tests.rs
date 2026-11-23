//! Unit tests for `pustman.rs`'s own wires: the wiring globals' bookkeeping, which the
//! `tests/` integration tests cannot see (the structs are module-private).

use gpui::{AppContext, TestAppContext};
use workspace::Workspace;

use crate::{HistoryPanelWiring, StoreLocation, init_with_store};

#[gpui::test]
fn history_panel_wiring_forgets_a_released_workspace(cx: &mut TestAppContext) {
    cx.update(|cx| init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let workspace_id = workspace.entity_id();
    cx.update(|window, _cx| window.activate_window());
    // The store opens on a background task; the panel registers once both exist.
    cx.run_until_parked();

    let registered = cx.read_global::<HistoryPanelWiring, _>(|wiring, _cx| {
        wiring.registered.contains(&workspace_id)
    });
    assert!(
        registered,
        "the workspace is registered once its History panel is in place (REQ-HIST-01)"
    );

    // The window closes with its workspace — the registration must go with it, the
    // `restore_wiring` shape (issue #163). The test's own handle goes first, so the
    // `remove_window` update is what drops the last strong reference and GPUI's effect
    // flush runs the release observers.
    drop(workspace);
    cx.update(|window, _cx| window.remove_window());
    cx.run_until_parked();

    let registered = cx.read_global::<HistoryPanelWiring, _>(|wiring, _cx| {
        wiring.registered.contains(&workspace_id)
    });
    assert!(
        !registered,
        "the wiring trims a released workspace's id instead of growing per window \
         (issue #163)"
    );
}
