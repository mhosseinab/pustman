//! The git wire (S44w): `pustman`'s init order calls `git::init`, so every workspace
//! window's status bar carries the branch item (REQ-COLLAB-03) — the readout the running
//! app shows inside a repository.

use gpui::TestAppContext;
use pustman::StoreLocation;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_git_status_item_registered(cx: &mut TestAppContext) {
    // The app's real init entry (`main.rs` runs `pustman::init`) and one workspace window.
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    draw_window(cx);

    // The status bar's left half carries the console count and the git branch item:
    // without `git::init` in the init order the strip holds only the console count.
    let left_items = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.status_bar().read(cx).left_items().len()
        })
    });
    assert_eq!(
        left_items, 2,
        "the init order wires the git branch item into the status bar (REQ-COLLAB-03)"
    );

    // Rendered-side proof: the readout the `git` crate labels renders inside the status
    // bar (its selector is the one `git`'s item puts on its element).
    let status_bar = cx
        .debug_bounds("status-bar")
        .expect("the status bar renders");
    let branch = cx
        .debug_bounds("status-git-branch")
        .expect("the git branch item renders in the status bar");
    assert!(
        branch.origin.x >= status_bar.origin.x
            && branch.origin.x + branch.size.width <= status_bar.origin.x + status_bar.size.width
            && branch.origin.y >= status_bar.origin.y
            && branch.origin.y + branch.size.height <= status_bar.origin.y + status_bar.size.height,
        "the git branch item renders inside the status bar ({branch:?} in {status_bar:?})"
    );
}
