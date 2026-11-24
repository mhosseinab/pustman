//! Move integrity (S40b fix 1): a move whose later write fails puts the disk back the way
//! it found it and says so in a toast, a retry repairs whatever a failed move left in the
//! parents' `order` arrays, and the two structural edges hold — a folder is never moved
//! into its own child, and a folder with children moves to another collection with its
//! subtree and its children's `order` intact. The pinned `reorder_tests.rs` stays one
//! test under `reorder::`; these live outside it so that filter still reports PASS 1.
//!
//! The failing write is the real one: the old parent's file is made immutable
//! (`chattr +i`, `chflags uchg`), which reads fine but refuses the atomic rename that
//! replaces it — the only way a read-only file stops `workspace_store::save_item`, whose
//! writes rename a temp file over the target. Root ignores ordinary file permissions,
//! so a read-only mode cannot stop a write here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{
    AppContext, Bounds, Entity, Focusable, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use crate::tree::{CollectionsTree, container_order};

/// The "Outer" folder's file — the old parent whose write the failure test blocks.
const OUTER_PATH: &str = "collections/users-api/outer/folder.json";

/// The "Get user" request's file, where the fixture writes it.
const REQUEST_PATH: &str = "collections/users-api/outer/get-user.request.json";

/// The "Inner" folder's file — "Outer"'s child.
const INNER_PATH: &str = "collections/users-api/outer/inner/folder.json";

/// Where the request's file lands once it moves into "Target".
const MOVED_REQUEST_PATH: &str = "collections/admin-api/target/get-user.request.json";

/// Where the "Outer" folder's file lands once the whole folder moves into "Target".
const MOVED_OUTER_PATH: &str = "collections/admin-api/target/outer/folder.json";

/// The workspace folder under test, and the stable ids the fixture seeded it with.
struct Fixture {
    /// Held so the folder outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    outer_id: ItemId,
    request_id: ItemId,
    inner_id: ItemId,
    admin_id: ItemId,
    target_id: ItemId,
}

/// A request item as a workspace file would parse it, written under `dir`.
fn request_item(id: ItemId, name: &str, dir: &str) -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id,
            name: String::from(name),
            method: Method::Get,
            url: String::from("https://api.test/users"),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from(dir),
        path: None,
    }
}

/// A folder item holding `order` — the child ids in display order (REQ-WSP-01).
fn folder_item(id: ItemId, name: &str, order: Vec<ItemId>, dir: &str) -> Item {
    Item {
        body: ItemBody::Folder(Folder {
            id,
            name: String::from(name),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order,
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from(dir),
        path: None,
    }
}

/// A collection item holding `order`, written under `dir`.
fn collection_item(id: ItemId, name: &str, order: Vec<ItemId>, dir: &str) -> Item {
    Item {
        body: ItemBody::Collection(Collection {
            id,
            name: String::from(name),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from(dir),
        path: None,
    }
}

/// A workspace on disk: "Users API" orders "Outer" (which orders "Get user" and
/// "Inner"), and "Admin API" orders "Target" — the two collections a cross-collection
/// move needs. Written through `workspace_store::save_item`, the same door the moves
/// write through.
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    // Fixed ids so "Target" sorts before "Outer": when both claim one child, `parent_map`
    // keeps the first, which forces the pre-fix code down its same-parent path and makes
    // `moving_a_child_claimed_by_two_parents_leaves_one_claim` fail deterministically
    // without the fix (random ULIDs minted in one millisecond sort either way).
    let outer_id: ItemId = "01JZZZZZZZZZZZZZZZZZZZZZZZ"
        .parse()
        .expect("the fixed outer id is a valid ULID");
    let request_id = ItemId::new();
    let inner_id = ItemId::new();
    let admin_id = ItemId::new();
    let target_id: ItemId = "01J00000000000000000000000"
        .parse()
        .expect("the fixed target id is a valid ULID");

    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("Move fixture"),
            format_version: String::from(workspace_store::FORMAT_VERSION),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    };
    save_item(&root, &workspace_file).expect("the workspace file writes");

    let seeded = [
        collection_item(collection_id, "Users API", vec![outer_id], "collections"),
        folder_item(
            outer_id,
            "Outer",
            vec![request_id, inner_id],
            "collections/users-api",
        ),
        request_item(request_id, "Get user", "collections/users-api/outer"),
        folder_item(inner_id, "Inner", Vec::new(), "collections/users-api/outer"),
        collection_item(admin_id, "Admin API", vec![target_id], "collections"),
        folder_item(target_id, "Target", Vec::new(), "collections/admin-api"),
    ];
    for item in seeded {
        save_item(&root, &item).expect("the fixture items write");
    }

    let fixture = Fixture {
        _temp: temp,
        root,
        collection_id,
        outer_id,
        request_id,
        inner_id,
        admin_id,
        target_id,
    };
    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(REQUEST_PATH)),
        "the fixture sits where its slugs say it does"
    );
    assert_eq!(
        snapshot.path(&fixture.outer_id),
        Some(Path::new(OUTER_PATH)),
        "the fixture sits where its slugs say it does"
    );
    fixture
}

/// The tree the way the shell shows it: a workspace window with the panel in its left
/// dock, every container open so all six rows are on screen for the drags.
fn open_tree<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<CollectionsTree>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::navigation::init(cx);
        crate::crud::init(cx);
        crate::reorder::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let tree = cx.update(|window, cx| {
        let snapshot = workspace_store::open(&fixture.root).expect("the seeded workspace opens");
        let tree = cx.new(|cx| CollectionsTree::new(snapshot, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(tree.clone(), window, cx);
        });
        tree
    });
    cx.update(|window, cx| {
        let focus_handle = tree.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    cx.update(|_window, cx| {
        tree.update(cx, |tree, cx| {
            for id in [
                fixture.collection_id,
                fixture.outer_id,
                fixture.inner_id,
                fixture.admin_id,
                fixture.target_id,
            ] {
                tree.set_expanded(id, true, cx);
            }
        });
    });
    draw_window(cx);
    (workspace, tree, cx)
}

/// The workspace as the disk holds it now.
fn disk_snapshot(fixture: &Fixture) -> WorkspaceSnapshot {
    workspace_store::open(&fixture.root).expect("the workspace still opens")
}

/// The ids `parent`'s `order` array holds (REQ-WSP-01).
fn order_of(snapshot: &WorkspaceSnapshot, parent: ItemId) -> Vec<ItemId> {
    snapshot
        .items
        .get(&parent)
        .and_then(|item| container_order(&item.body).cloned())
        .unwrap_or_default()
}

/// The on-screen bounds of the row named `name`. Rows are keyed by item id, so the
/// lookup leaks the row's selector once — `debug_bounds` addresses a `&'static str`.
fn row_bounds(
    name: &str,
    tree: &Entity<CollectionsTree>,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let selector = cx.read(|cx| {
        tree.read_with(cx, |tree, _| {
            tree.rows()
                .iter()
                .find(|row| row.name() == name)
                .map(|row| row.element_id().to_string())
        })
    });
    let selector = selector.unwrap_or_else(|| panic!("the “{name}” row is in the tree"));
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("the “{name}” row is on screen"))
}

/// Drag the row at `from` so the pointer lands on `to`: mouse down, one move that
/// crosses GPUI's drag threshold (that move starts the drag), then the move that lands
/// on the drop zone — its hover event is what tells the tree where the drop would land —
/// and the release that drops it.
fn drag_to(cx: &mut VisualTestContext, from: Bounds<Pixels>, to: Point<Pixels>) {
    let start = from.center();
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(start.x, start.y + px(4.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    draw_window(cx);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    draw_window(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The point in a row's middle — its "drop into" zone.
fn into_point(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.center().x, bounds.top() + bounds.size.height / 2.)
}

/// Whether `message` is the one toast on the workspace (the `delete_failure_tests.rs`
/// shape: one toast, under the message's own test selector).
fn toast_shows(workspace: &Entity<Workspace>, cx: &mut VisualTestContext, message: &str) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(message).leak();
    let one_toast = cx
        .read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
        == 1;
    one_toast && cx.debug_bounds(message_selector).is_some()
}

/// Arm (or with [`None`] disarm) the seam that refuses one of a move's writes — fix 2's
/// failure injection, hermetic where a filesystem trick wants root or a special disk.
fn refuse_write(tree: &Entity<CollectionsTree>, nth: Option<usize>, cx: &mut VisualTestContext) {
    cx.update(|_window, cx| {
        tree.update(cx, |tree, _| tree.reorder.refuse_the_nth_write(nth));
    });
}

/// The second file a rename/rename merge leaves carrying one id (REQ-COLLAB-02): the
/// same request under another name, written beside the fixture's own copy — which
/// `workspace_store::save_item` allows and `workspace_store::open` then reports as a
/// duplicate. It sorts first, so it is the copy the tree draws and the move acts on.
fn write_merge_copy(fixture: &Fixture, id: ItemId) {
    save_item(
        &fixture.root,
        &request_item(id, "Fetch user", "collections/users-api/outer"),
    )
    .expect("the merge copy writes");
}

/// The stale state a failed move leaves behind: `parent` claims `child` in its `order`
/// while the parent that holds the child's file lists it too.
fn claim_child(fixture: &Fixture, parent: ItemId, child: ItemId) {
    let snapshot = disk_snapshot(fixture);
    let mut item = snapshot
        .items
        .get(&parent)
        .cloned()
        .expect("the parent is in the workspace");
    match &mut item.body {
        ItemBody::Folder(folder) => folder.order.push(child),
        _ => panic!("the fixture parent is a folder"),
    }
    save_item(&fixture.root, &item).expect("the stale claim writes");
}

/// The toast the refused write shows: what could not be written, which file it wanted
/// to replace, and the seam's own reason.
fn failure_message(name: &str) -> String {
    format!("Could not take “{name}” out of “Outer” ({OUTER_PATH}): io: the write was refused")
}

#[gpui::test]
fn a_failed_move_rolls_back_and_the_retry_reclaims_the_child(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (workspace, tree, cx) = open_tree(&fixture, cx);

    // The move's third write — the old parent's `order` — is refused, so the first two
    // have landed and the rollback has real work to undo.
    refuse_write(&tree, Some(3), cx);

    let request = row_bounds("Get user", &tree, cx);
    let target = row_bounds("Target", &tree, cx);
    drag_to(cx, request, into_point(target));

    let snapshot = disk_snapshot(&fixture);
    assert!(
        fixture.root.join(REQUEST_PATH).exists(),
        "the request's file is back where the move found it"
    );
    assert!(
        !fixture.root.join(MOVED_REQUEST_PATH).exists(),
        "and the copy the move wrote at the new place is put back with it"
    );
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(REQUEST_PATH)),
        "the disk is as before the move"
    );
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.request_id, fixture.inner_id],
        "the old parent's order array is untouched"
    );
    assert_eq!(
        order_of(&snapshot, fixture.target_id),
        Vec::new(),
        "the destination's order array is put back the way it was"
    );
    assert!(
        toast_shows(&workspace, cx, &failure_message("Get user")),
        "and the failure reaches the user as a toast naming the path and the reason"
    );

    // Retry with the writes allowed: the move lands, and exactly one parent lists the
    // child.
    refuse_write(&tree, None, cx);
    let request = row_bounds("Get user", &tree, cx);
    let target = row_bounds("Target", &tree, cx);
    drag_to(cx, request, into_point(target));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.target_id),
        vec![fixture.request_id],
        "the retried move joins the destination's order"
    );
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.inner_id],
        "and leaves the old parent's order"
    );
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(MOVED_REQUEST_PATH)),
        "and the file follows it (REQ-WSP-01)"
    );
    assert!(
        fixture.root.join(MOVED_REQUEST_PATH).exists(),
        "and it is on disk, not only in the snapshot"
    );
}

#[gpui::test]
fn moving_a_folder_into_its_own_child_is_refused(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (workspace, tree, cx) = open_tree(&fixture, cx);

    let outer = row_bounds("Outer", &tree, cx);
    let inner = row_bounds("Inner", &tree, cx);
    drag_to(cx, outer, into_point(inner));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.request_id, fixture.inner_id],
        "the refused move changed no order array"
    );
    assert_eq!(
        order_of(&snapshot, fixture.inner_id),
        Vec::new(),
        "and the child it was refused into is untouched"
    );
    assert!(
        fixture.root.join(OUTER_PATH).exists() && fixture.root.join(INNER_PATH).exists(),
        "and no file moved"
    );
    assert!(
        !fixture
            .root
            .join("collections/users-api/outer/inner/outer")
            .exists(),
        "the folder did not end up inside its own child"
    );
    assert!(
        toast_shows(
            &workspace,
            cx,
            "Could not move “Outer”: it cannot go inside itself"
        ),
        "the refusal reaches the user as a toast"
    );
}

#[gpui::test]
fn moving_a_folder_with_children_to_another_collection_moves_the_subtree(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    let outer = row_bounds("Outer", &tree, cx);
    let target = row_bounds("Target", &tree, cx);
    drag_to(cx, outer, into_point(target));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        snapshot.path(&fixture.outer_id),
        Some(Path::new(MOVED_OUTER_PATH)),
        "the folder's own file moved to the other collection"
    );
    assert!(
        fixture
            .root
            .join("collections/admin-api/target/outer/get-user.request.json")
            .exists(),
        "its request child came along"
    );
    assert!(
        fixture
            .root
            .join("collections/admin-api/target/outer/inner/folder.json")
            .exists(),
        "and its folder child — the whole slug directory moved"
    );
    assert!(
        !fixture.root.join("collections/users-api/outer").exists(),
        "the old slug directory is gone — the move took it wholesale"
    );
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.request_id, fixture.inner_id],
        "the moved folder keeps its children's order"
    );
    assert_eq!(
        order_of(&snapshot, fixture.target_id),
        vec![fixture.outer_id],
        "and the destination orders it"
    );
}

#[gpui::test]
fn a_rolled_back_move_keeps_the_merge_artifacts_other_copy(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    write_merge_copy(&fixture, fixture.request_id);
    let (workspace, tree, cx) = open_tree(&fixture, cx);
    refuse_write(&tree, Some(3), cx);

    // The kept copy of the id — the merge copy, which sorts first — is the row that
    // drags; its twin stays where the merge left it.
    let request = row_bounds("Fetch user", &tree, cx);
    let target = row_bounds("Target", &tree, cx);
    drag_to(cx, request, into_point(target));

    let snapshot = disk_snapshot(&fixture);
    assert!(
        fixture
            .root
            .join("collections/users-api/outer/fetch-user.request.json")
            .exists(),
        "the copy the move took is back where it was"
    );
    assert!(
        fixture.root.join(REQUEST_PATH).exists(),
        "and the other file carrying the id — the user's own merge artifact — is untouched"
    );
    assert!(
        !fixture
            .root
            .join("collections/admin-api/target/fetch-user.request.json")
            .exists(),
        "no copy of the move is left at the new place"
    );
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.request_id, fixture.inner_id],
        "the rollback restored the order arrays too"
    );
    assert_eq!(
        order_of(&snapshot, fixture.target_id),
        Vec::new(),
        "the destination is left as it was"
    );
    assert!(
        toast_shows(&workspace, cx, &failure_message("Fetch user")),
        "and the failure reaches the user as a toast"
    );
}

#[gpui::test]
fn moving_a_child_claimed_by_two_parents_leaves_one_claim(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    // The stale state a failed move leaves: both "Outer" and "Target" claim "Get user".
    claim_child(&fixture, fixture.target_id, fixture.request_id);
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    let request = row_bounds("Get user", &tree, cx);
    let target = row_bounds("Target", &tree, cx);
    drag_to(cx, request, into_point(target));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.target_id),
        vec![fixture.request_id],
        "the destination lists the child once"
    );
    assert_eq!(
        order_of(&snapshot, fixture.outer_id),
        vec![fixture.inner_id],
        "and the stale claim is gone — exactly one parent lists it"
    );
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(MOVED_REQUEST_PATH)),
        "and the file follows the one claim that remains (REQ-WSP-01)"
    );
}
