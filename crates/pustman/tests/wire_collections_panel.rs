//! The collections-panel wire (S39w): the app's init order must give every workspace
//! window the Collections panel in its left dock (PRD REQ-UI-01), so the running app
//! shows the collections tree.

use collections_panel::{CollectionsTree, PANEL_ID};
use gpui::TestAppContext;
use pustman::{StartupProblems, StoreLocation};
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_collections_panel_registered_in_left_dock(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // The panel registers when its workspace appears, deferred off the workspace's own
    // update; the store opens on a background task in the same window of time.
    cx.run_until_parked();
    draw_window(cx);

    let registered: Vec<&'static str> = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panels()
                .iter()
                .map(|panel| panel.panel_id(cx))
                .collect()
        })
    });
    assert!(
        registered.contains(&PANEL_ID),
        "the left dock holds the Collections panel once the window exists (REQ-UI-01); \
         registered panels: {registered:?}"
    );

    // The registered entry is the collections tree itself — `pustman::init` wires the
    // panel `collections_panel::init` builds, not some other view under that id. The
    // dock's *active* panel is deliberately not asserted: the first panel added wins and
    // the History panel registers against the same dock once the store lands, an order
    // this test does not control.
    let registered_tree = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .left_dock()
                .read(cx)
                .panels()
                .iter()
                .any(|panel| panel.to_any_view().downcast::<CollectionsTree>().is_ok())
        })
    });
    assert!(
        registered_tree,
        "the left dock's Collections entry is the collections tree view"
    );
}
