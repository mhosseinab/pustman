//! Drag to reorder and move (S40b), driven the way the user drives it: a real workspace
//! folder on disk (the promise is about `order` arrays and files), a workspace window
//! with the panel in its left dock, and real mouse drags plus the keyboard move keys.
//!
//! The step pins this file to one behavior test — `reorder_updates_parent_order_array` —
//! so the drag to reorder, the drag into a folder and the keyboard move all walk through
//! it as phases of the one promise: a move updates the parents' `order` arrays on disk.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{
    AppContext, Bounds, Focusable, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};
use tempfile::TempDir;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use crate::tree::{CollectionsTree, TreeRow, container_order};

/// The collection's file, as the fixture writes it: slug of "Users API" (REQ-WSP-03).
const COLLECTION_PATH: &str = "collections/users-api/collection.json";

/// The first request's file, as the fixture writes it: slug of "Get user".
const FIRST_REQUEST_PATH: &str = "collections/users-api/get-user.request.json";

/// The second request's file, as the fixture writes it: slug of "List users".
const SECOND_REQUEST_PATH: &str = "collections/users-api/list-users.request.json";

/// The folder's file, as the fixture writes it: slug of "Admin".
const FOLDER_PATH: &str = "collections/users-api/admin/folder.json";

/// Where the second request's file lands once it moves into the "Admin" folder.
const MOVED_REQUEST_PATH: &str = "collections/users-api/admin/list-users.request.json";

/// The workspace folder under test, and the stable ids the fixture seeded it with.
struct Fixture {
    /// Held so the folder outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    first_id: ItemId,
    second_id: ItemId,
    folder_id: ItemId,
}

/// A workspace on disk — `pustman.json`, one collection "Users API" whose `order` runs
/// "Get user", "List users", "Admin" — written through `workspace_store::save_item`, the
/// same door the moves write through (REQ-WSP-01).
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    let first_id = ItemId::new();
    let second_id = ItemId::new();
    let folder_id = ItemId::new();

    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("Reorder fixture"),
            format_version: String::from(workspace_store::FORMAT_VERSION),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    };
    save_item(&root, &workspace_file).expect("the workspace file writes");

    let collection_file = Item {
        body: ItemBody::Collection(Collection {
            id: collection_id,
            name: String::from("Users API"),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order: vec![first_id, second_id, folder_id],
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    };
    let collection_path = save_item(&root, &collection_file).expect("the collection writes");
    assert_eq!(
        collection_path,
        Path::new(COLLECTION_PATH),
        "the fixture sits where its slugs say it does"
    );

    let request_path = |id: ItemId, name: &str| Item {
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
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };
    let first_path =
        save_item(&root, &request_path(first_id, "Get user")).expect("the request writes");
    assert_eq!(
        first_path,
        Path::new(FIRST_REQUEST_PATH),
        "the fixture sits where its slugs say it does"
    );
    let second_path =
        save_item(&root, &request_path(second_id, "List users")).expect("the request writes");
    assert_eq!(
        second_path,
        Path::new(SECOND_REQUEST_PATH),
        "the fixture sits where its slugs say it does"
    );

    let folder_file = Item {
        body: ItemBody::Folder(Folder {
            id: folder_id,
            name: String::from("Admin"),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };
    let folder_path = save_item(&root, &folder_file).expect("the folder writes");
    assert_eq!(
        folder_path,
        Path::new(FOLDER_PATH),
        "the fixture sits where its slugs say it does"
    );

    Fixture {
        _temp: temp,
        root,
        collection_id,
        first_id,
        second_id,
        folder_id,
    }
}

/// The tree the way the shell shows it: a workspace window with the panel in its left
/// dock and the tree focused — a keystroke reaches a handler only after a paint
/// registered its focus path.
fn open_tree<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (
    gpui::Entity<Workspace>,
    gpui::Entity<CollectionsTree>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `navigation::init` the arrows cannot move the selection and
        // without `reorder::init` the move keys are unbound.
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
    draw_window(cx);
    (workspace, tree, cx)
}

/// The rows' names, in draw order.
fn row_names(tree: &gpui::Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<String> {
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
    tree: &gpui::Entity<CollectionsTree>,
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
/// on the drop zone — its hover event is what tells the tree where the drop would
/// land — and the release that drops it.
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

/// The point in a row's top strip — its "drop before" zone.
fn before_point(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.center().x, bounds.top() + px(2.))
}

/// The point in a row's middle — its "drop into" zone on a container.
fn into_point(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.center().x, bounds.top() + bounds.size.height / 2.)
}

#[gpui::test]
fn reorder_updates_parent_order_array(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API"],
        "the fixture starts with one collapsed collection"
    );

    // Down selects the collection and Right opens it: the rows the drags act on.
    cx.simulate_keystrokes("down right");
    draw_window(cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Get user", "List users", "Admin"]
    );

    // Phase 1 — drag to reorder: "List users" onto "Get user"'s top strip drops it
    // before its sibling, and the collection's `order` array on disk says so.
    let list_users = row_bounds("List users", &tree, cx);
    let get_user = row_bounds("Get user", &tree, cx);
    drag_to(cx, list_users, before_point(get_user));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.collection_id),
        vec![fixture.second_id, fixture.first_id, fixture.folder_id],
        "the drag rewrote the parent's order array on disk"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "List users", "Get user", "Admin"],
        "and the tree draws the new order"
    );

    // Phase 2 — drag to move into a folder: "List users" onto "Admin"'s middle drops it
    // into the folder — its `order` entry moves between parents and its file follows.
    let list_users = row_bounds("List users", &tree, cx);
    let admin = row_bounds("Admin", &tree, cx);
    drag_to(cx, list_users, into_point(admin));

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.collection_id),
        vec![fixture.first_id, fixture.folder_id],
        "the moved request left the collection's order"
    );
    assert_eq!(
        order_of(&snapshot, fixture.folder_id),
        vec![fixture.second_id],
        "and joined the folder's order"
    );
    assert_eq!(
        snapshot.path(&fixture.second_id),
        Some(Path::new(MOVED_REQUEST_PATH)),
        "and its file moved into the folder's slug directory (REQ-WSP-01)"
    );
    assert!(
        fixture.root.join(MOVED_REQUEST_PATH).exists(),
        "and it is on disk, not only in the snapshot"
    );
    assert!(
        !fixture.root.join(SECOND_REQUEST_PATH).exists(),
        "the old file is gone — the move took it"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Get user", "Admin", "List users"],
        "and the tree draws it under the just-expanded folder"
    );

    // Phase 3 — the keyboard's twin of a drag: the selection is on "Admin" and the
    // move-up key reorders it exactly as dragging it above "Get user" would.
    cx.simulate_keystrokes("down down alt-up");
    cx.run_until_parked();
    draw_window(cx);

    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        order_of(&snapshot, fixture.collection_id),
        vec![fixture.folder_id, fixture.first_id],
        "the keyboard move rewrote the parent's order array too"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Admin", "List users", "Get user"],
        "and the tree draws the new order"
    );
}
