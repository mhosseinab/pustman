//! The external feed's lifecycle (S42a fix 1/2): closing a window stops its watcher —
//! no orphaned notify thread, poll task or re-reads for nobody — and a workspace that
//! cannot be read shows one "could not re-read" toast, not one per change batch, cleared
//! as soon as the workspace reads again. Wired from `collections_panel.rs` (not
//! `external.rs`) so the pinned `external::` filter still reports PASS 1.

use std::path::Path;
use std::time::Duration;

use api_model::ItemId;
use gpui::{Entity, TestAppContext, VisualTestContext, WeakEntity};
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
/// "Pets".
fn fixture_workspace(root: &Path) {
    write_json(
        &root.join("pustman.json"),
        serde_json::json!({
            "id": ItemId::new().to_string(),
            "name": "Feed lifecycle workspace",
            "format_version": "1.0",
        }),
    );
    write_json(
        &root.join("collections/pets/collection.json"),
        collection_json(ItemId::new(), "Pets"),
    );
}

/// One window on the app's real init order: `workspace::init` gives it its
/// [`WorkspaceOpen`], `crate::init` registers the tree and the feed beside it.
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

/// The collections tree `crate::init` registered in `workspace`'s left dock.
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
/// ([`WorkspaceOpen::open_folder`]).
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

/// The toasts on this window's workspace (the shell's toast stack is "how tests read
/// it").
fn toast_count(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> usize {
    cx.read(|cx| workspace.read_with(cx, |workspace, _| workspace.toasts().read(cx).len()))
}

/// Bounded wait for the toast count to reach `want`: the watcher is a real `notify`
/// thread, so this passes real time (the scheduler parks through it) but gives up after
/// ~2 s instead of hanging.
async fn wait_for_toast_count(
    workspace: &Entity<Workspace>,
    want: usize,
    cx: &mut VisualTestContext,
) -> bool {
    for _ in 0..20 {
        if toast_count(workspace, cx) == want {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// A short real-time settle: long enough for a change batch to be polled and re-read,
/// for the "nothing happened" assertions (the watcher's 100 ms debounce + 100 ms poll).
async fn settle(cx: &mut VisualTestContext) {
    for _ in 0..5 {
        cx.executor().timer(Duration::from_millis(100)).await;
    }
}

#[gpui::test]
async fn closing_a_window_stops_its_watcher(cx: &mut TestAppContext) {
    // The watcher under test is real IO over a real tempdir: parking lets real time
    // pass through the GPUI timers the waits below use.
    cx.executor().allow_parking();

    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path();
    fixture_workspace(root);

    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, root, cx);
    let tree = collections_tree(&workspace, cx);
    let released_tree: WeakEntity<CollectionsTree> = tree.downgrade();
    assert_eq!(
        row_names(&tree, cx),
        vec!["Pets"],
        "the feed works while the window is open"
    );

    // The window closes: the test's own handles go first, so the window update is what
    // drops the last strong references (the `pustman_tests` shape) and its effect flush
    // plus `run_until_parked` run the release observers.
    drop(tree);
    drop(workspace);
    cx.update(|_, _| {});
    cx.update(|window, _cx| window.remove_window());
    cx.run_until_parked();

    assert!(
        released_tree.upgrade().is_none(),
        "the tree is released with its window, not kept alive by its feed (S42a fix 1)"
    );
    assert_eq!(
        cx.read(crate::external::live_feeds),
        0,
        "the feed goes with its tree: no watcher, pump or subscription outlives the window"
    );

    let reads_before = cx.read(|_cx| crate::external::disk_reads(root));
    write_json(
        &root.join("collections/payments/collection.json"),
        collection_json(ItemId::new(), "Payments"),
    );
    settle(cx).await;
    assert_eq!(
        cx.read(|_cx| crate::external::disk_reads(root)),
        reads_before,
        "a disk change after the close triggers no re-read (S42a fix 1)"
    );
}

#[gpui::test]
async fn a_read_failure_toasts_once_until_the_workspace_reads_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path();
    fixture_workspace(root);

    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, root, cx);
    let _tree = collections_tree(&workspace, cx);

    // The workspace stops being readable behind the app's back — the renamed-away
    // `pustman.json` of a branch switch mid-flight.
    std::fs::rename(root.join("pustman.json"), root.join("pustman.json.pulled"))
        .expect("the workspace file moves");
    assert!(
        wait_for_toast_count(&workspace, 1, cx).await,
        "the unreadable workspace reaches the user once; toasts: {}",
        toast_count(&workspace, cx)
    );

    // Later change batches must not stack another copy of the same failure.
    write_json(
        &root.join("collections/payments/collection.json"),
        collection_json(ItemId::new(), "Payments"),
    );
    settle(cx).await;
    assert_eq!(
        toast_count(&workspace, cx),
        1,
        "one toast stands for the whole failure streak (S42a fix 2)"
    );

    // The workspace reads again: the problem is gone, so its toast goes.
    std::fs::rename(root.join("pustman.json.pulled"), root.join("pustman.json"))
        .expect("the workspace file comes back");
    assert!(
        wait_for_toast_count(&workspace, 0, cx).await,
        "the next successful read clears the failure toast (S42a fix 2); toasts: {}",
        toast_count(&workspace, cx)
    );

    // And a later failure reports again: the clear reset the state, not muted it.
    std::fs::rename(root.join("pustman.json"), root.join("pustman.json.pulled"))
        .expect("the workspace file moves again");
    assert!(
        wait_for_toast_count(&workspace, 1, cx).await,
        "a new failure streak shows its toast again; toasts: {}",
        toast_count(&workspace, cx)
    );
}
