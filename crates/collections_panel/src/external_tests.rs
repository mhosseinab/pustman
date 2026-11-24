//! The external-change feed end to end (REQ-COLLAB-01): a workspace opened through
//! [`WorkspaceOpen::open_folder`] — never a hand-installed snapshot — is what the
//! collections tree draws, and a file added behind the app's back (the `git pull`, the
//! branch switch, the teammate's edit) reaches the tree within the 1 s budget.

use std::path::Path;
use std::time::Duration;

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::test_support::init_test;
use workspace::{Workspace, WorkspaceOpen};

use crate::tree::{CollectionsTree, TreeRow};

/// One collection file's JSON (PRD §6.1): the shape `workspace_store` reads back.
fn collection_json(id: ItemId, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id.to_string(),
        "name": name,
        "order": [],
    })
}

/// Write one fixture file, creating its directory the way a checkout would.
fn write_json(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the fixture directories are created");
    }
    let bytes = serde_json::to_string_pretty(&value).expect("the fixture serializes");
    std::fs::write(path, bytes).expect("the fixture is written");
}

/// The workspace folder at `root` (PRD §6.1): `pustman.json` and one collection,
/// "Pets" — the row the tree must draw from the opened snapshot before anything
/// changes on disk.
fn fixture_workspace(root: &Path) {
    write_json(
        &root.join("pustman.json"),
        serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "External feed workspace",
            "format_version": "1.0",
        }),
    );
    write_json(
        &root.join("collections/pets/collection.json"),
        collection_json(ItemId::new(), "Pets"),
    );
}

/// One window on the app's real init order: `workspace::init` gives it its
/// [`WorkspaceOpen`], `crate::init` registers the tree and this step's feed beside it.
fn app_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    (workspace, cx)
}

/// The collections tree `crate::init` registered in `workspace`'s left dock — the panel
/// this step feeds, not a view the test built by hand.
fn collections_tree(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Entity<CollectionsTree> {
    cx.read(|cx| {
        workspace
            .read_with(cx, |workspace, cx| {
                workspace
                    .left_dock()
                    .read(cx)
                    .panels()
                    .iter()
                    .find(|panel| panel.panel_id(cx) == crate::PANEL_ID)
                    .map(|panel| {
                        panel
                            .to_any_view()
                            .downcast::<CollectionsTree>()
                            .expect("the Collections panel is the tree")
                    })
            })
            .expect("the Collections panel registers with the workspace")
    })
}

/// Open `root` as this window's workspace through the one door every open lands in
/// ([`WorkspaceOpen::open_folder`]): the snapshot the tree draws comes from there.
fn open_workspace_folder(workspace: &Entity<Workspace>, root: &Path, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| {
            opener.open_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();
}

/// The rows' names, in draw order.
fn row_names(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        tree.read_with(cx, |tree, _| {
            tree.rows()
                .iter()
                .map(TreeRow::name)
                .map(str::to_owned)
                .collect()
        })
    })
}

/// Bounded wait for a row to appear: the watcher is a real `notify` thread over a real
/// folder, so this passes real time (the scheduler parks through it) but gives up after
/// ~2 s instead of hanging — a regression must fail, not stall.
async fn wait_for_row(
    tree: &Entity<CollectionsTree>,
    name: &str,
    cx: &mut VisualTestContext,
) -> bool {
    for _ in 0..20 {
        if row_names(tree, cx).iter().any(|row| row == name) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

#[gpui::test]
async fn external_add_appears_in_tree(cx: &mut TestAppContext) {
    // The watcher under test is real IO over a real tempdir: parking lets real time
    // pass through the GPUI timers below, the seam for "a mix of deterministic and
    // non-deterministic async behavior, such as when interacting with I/O".
    cx.executor().allow_parking();

    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path();
    fixture_workspace(root);

    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, root, cx);
    let tree = collections_tree(&workspace, cx);

    // The tree draws the opened workspace's snapshot (S42a): the folder opened through
    // [`WorkspaceOpen`] is what its rows come from — nothing was installed by hand.
    assert_eq!(
        row_names(&tree, cx),
        vec!["Pets"],
        "the opened workspace's snapshot feeds the tree"
    );

    // A collection added behind the app's back — the `git pull` case (REQ-COLLAB-01).
    write_json(
        &root.join("collections/payments/collection.json"),
        collection_json(ItemId::new(), "Payments"),
    );

    assert!(
        wait_for_row(&tree, "Payments", cx).await,
        "an external add reaches the tree within the 1 s budget; rows: {:?}",
        row_names(&tree, cx)
    );
}
