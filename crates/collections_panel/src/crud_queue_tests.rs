//! The queue's promise (S40a fix 2): a verb queued behind another acts on the workspace
//! as the disk holds it when its write runs — never on the snapshot its row was read
//! from. A rename that moved a subtree, or a delete that removed it, must be visible to
//! the verb behind it: its files go (or land) where they are NOW.
//!
//! Each test fires two verbs back to back through the same handlers the keys, the row
//! menu and the confirm modal call — one `update`, so no write lands between them — and
//! then lets the queue drain. These live in their own file wired from [`crate`] (not
//! from `crud::`), so the step's pinned `crud::` filter still selects exactly three.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use crate::crud::ChildKind;
use crate::tree::CollectionsTree;

/// The directory the renamed folder leaves behind — the one a stale write would
/// recreate.
const OLD_FOLDER_DIR: &str = "collections/users-api/sessions";

/// Where the rename moves that folder to.
const NEW_FOLDER_DIR: &str = "collections/users-api/renamed";

/// The workspace folder under test and the ids the fixture seeded: collection "Users
/// API" → folder "Sessions" → folder "Inner" → request "Login" (REQ-WSP-01's `order`
/// arrays the whole way down).
struct Fixture {
    /// Held so the folder outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    folder_id: ItemId,
    inner_id: ItemId,
    request_id: ItemId,
}

/// The four-level workspace on disk, written through `workspace_store::save_item`.
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    let folder_id = ItemId::new();
    let inner_id = ItemId::new();
    let request_id = ItemId::new();

    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("CRUD queue fixture"),
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
            order: vec![folder_id],
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    };
    save_item(&root, &collection_file).expect("the collection writes");

    let folder_file = Item {
        body: ItemBody::Folder(Folder {
            id: folder_id,
            name: String::from("Sessions"),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order: vec![inner_id],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };
    save_item(&root, &folder_file).expect("the folder writes");

    let inner_file = Item {
        body: ItemBody::Folder(Folder {
            id: inner_id,
            name: String::from("Inner"),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order: vec![request_id],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections/users-api/sessions"),
        path: None,
    };
    save_item(&root, &inner_file).expect("the inner folder writes");

    let request_file = Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: request_id,
            name: String::from("Login"),
            method: Method::Post,
            url: String::from("https://api.test/login"),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections/users-api/sessions/inner"),
        path: None,
    };
    save_item(&root, &request_file).expect("the request writes");

    Fixture {
        _temp: temp,
        root,
        collection_id,
        folder_id,
        inner_id,
        request_id,
    }
}

/// The tree the way the shell shows it: a workspace window, the panel in its left dock,
/// every key binding wired and the tree focused.
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
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    // A real window is active; a test window is not until asked.
    cx.update(|window, _cx| window.activate_window());
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

/// The workspace as the disk holds it now.
fn disk_snapshot(fixture: &Fixture) -> WorkspaceSnapshot {
    workspace_store::open(&fixture.root).expect("the workspace still opens")
}

/// The ids `order` holds for `container` (REQ-WSP-01).
fn order_of(snapshot: &WorkspaceSnapshot, container: ItemId) -> Vec<ItemId> {
    match snapshot.items.get(&container).map(|item| &item.body) {
        Some(ItemBody::Collection(collection)) => collection.order.clone(),
        Some(ItemBody::Folder(folder)) => folder.order.clone(),
        _ => Vec::new(),
    }
}

#[gpui::test]
fn queued_delete_removes_the_file_wherever_the_rename_moved_it(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // The rename of the folder and the delete of its request, back to back: the delete
    // runs after the rename moved everything under `sessions/` to `renamed/`.
    cx.update(|window, cx| {
        tree.update(cx, |tree, cx| {
            tree.commit_rename(fixture.folder_id, String::from("Renamed"), window, cx);
            tree.confirm_delete(fixture.request_id, window, cx);
        });
    });
    cx.run_until_parked();

    assert!(
        !fixture.root.join(OLD_FOLDER_DIR).exists(),
        "the rename left no old directory"
    );
    assert!(
        !fixture
            .root
            .join(NEW_FOLDER_DIR)
            .join("inner/login.request.json")
            .exists(),
        "the delete removed the child's file where the rename moved it"
    );
    let snapshot = disk_snapshot(&fixture);
    assert!(
        !snapshot.items.contains_key(&fixture.request_id),
        "the request is gone from the workspace"
    );
    assert_eq!(
        order_of(&snapshot, fixture.inner_id),
        Vec::<ItemId>::new(),
        "and its parent no longer orders it"
    );
    assert!(
        snapshot.items.contains_key(&fixture.inner_id),
        "the folder the delete ran inside survives"
    );
}

#[gpui::test]
fn queued_duplicate_lands_next_to_the_renamed_folder(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // The rename of the outer folder and the duplicate of the folder inside it, back to
    // back: the copy must follow the subtree into `renamed/`, not resurrect `sessions/`.
    cx.update(|window, cx| {
        tree.update(cx, |tree, cx| {
            tree.commit_rename(fixture.folder_id, String::from("Renamed"), window, cx);
            tree.duplicate_item(fixture.inner_id, window, cx);
        });
    });
    cx.run_until_parked();

    assert!(
        !fixture.root.join(OLD_FOLDER_DIR).exists(),
        "no old directory is recreated by the copy"
    );
    assert!(
        fixture
            .root
            .join(NEW_FOLDER_DIR)
            .join("copy-of-inner/folder.json")
            .exists(),
        "the copy sits next to its original under the renamed folder"
    );
    assert!(
        fixture
            .root
            .join(NEW_FOLDER_DIR)
            .join("copy-of-inner/login.request.json")
            .exists(),
        "and its subtree is copied with it"
    );
    let snapshot = disk_snapshot(&fixture);
    let copies: Vec<(ItemId, Vec<ItemId>)> = snapshot
        .items
        .iter()
        .filter(|(_, item)| item.name() == "Copy of Inner")
        .map(|(id, _)| (*id, order_of(&snapshot, *id)))
        .collect();
    assert_eq!(copies.len(), 1, "exactly one copy of the folder");
    let (copy_id, copy_order) = copies
        .first()
        .cloned()
        .expect("the copy is in the snapshot");
    assert_ne!(copy_id, fixture.inner_id, "the copy is a new artifact");
    assert_eq!(copy_order.len(), 1, "the copy's order is remapped");
    let copied_child = copy_order[0];
    assert_ne!(copied_child, fixture.request_id, "no id is shared");
}

#[gpui::test]
fn queued_create_behind_a_delete_writes_nothing(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // The confirmed delete of the folder and a create inside it, back to back: the
    // create must not resurrect the directory the delete just removed.
    cx.update(|window, cx| {
        tree.update(cx, |tree, cx| {
            tree.confirm_delete(fixture.folder_id, window, cx);
            tree.create_child(Some(fixture.folder_id), ChildKind::Request, window, cx);
        });
    });
    cx.run_until_parked();

    assert!(
        !fixture.root.join(OLD_FOLDER_DIR).exists(),
        "no file and no directory: the create wrote nothing into the deleted folder"
    );
    let snapshot = disk_snapshot(&fixture);
    assert!(
        !snapshot
            .items
            .values()
            .any(|item| item.name() == "New Request"),
        "and no new request exists anywhere in the workspace"
    );
    assert_eq!(
        order_of(&snapshot, fixture.collection_id),
        Vec::<ItemId>::new(),
        "the collection's order holds only what survived"
    );
}
