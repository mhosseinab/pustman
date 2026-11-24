//! The CRUD write-integrity guarantees (S40a fix 1): writes are serialized per tree and
//! apply to the parent's *current* `order` as the disk holds it, a duplicate copies a
//! subtree without ever claiming the source's children, a container delete takes its
//! whole subtree with it, and the inline rename backs out when the field loses focus.
//!
//! Driven the way the shell drives it — a real workspace folder on disk, a workspace
//! window with the panel in its left dock, keyboard and confirm modal — so the
//! assertions are about files and rows, not internals. These live in their own file
//! wired from [`crate`] (not from `crud::`), so the step's pinned `crud::` filter still
//! selects exactly the three tests it names.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Auth, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use crate::crud::{DELETE_CONFIRM_BUTTON_SELECTOR, DeleteItemModal};
use crate::tree::CollectionsTree;

/// The collection's file, as the fixture writes it: slug of "Users API" (REQ-WSP-03).
const COLLECTION_PATH: &str = "collections/users-api/collection.json";

/// The folder's file: slug of "Sessions", inside the collection's slug directory.
const FOLDER_PATH: &str = "collections/users-api/sessions/folder.json";

/// The request's file: slug of "Login", inside the folder's slug directory.
const REQUEST_PATH: &str = "collections/users-api/sessions/login.request.json";

/// The directory a duplicate of "Sessions" lands in: "Copy of Sessions" slugged.
const COPY_DIR: &str = "collections/users-api/copy-of-sessions";

/// How many slug spellings `workspace_store` tries before a name collides for good —
/// the fixture fills every one of them to make a child copy fail (REQ-WSP-03).
const SLUG_ATTEMPTS: u32 = 999;

/// The workspace folder under test and the stable ids the fixture seeded it with:
/// collection "Users API" → folder "Sessions" → request "Login" (REQ-WSP-01's `order`
/// arrays the whole way down).
struct Fixture {
    /// Held so the folder outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    folder_id: ItemId,
    request_id: ItemId,
}

/// The three-level workspace on disk, written through `workspace_store::save_item` —
/// the same door the CRUD verbs use.
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    let folder_id = ItemId::new();
    let request_id = ItemId::new();

    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("CRUD integrity fixture"),
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
    let collection_path = save_item(&root, &collection_file).expect("the collection writes");
    assert_eq!(collection_path, Path::new(COLLECTION_PATH));

    let folder_file = Item {
        body: ItemBody::Folder(Folder {
            id: folder_id,
            name: String::from("Sessions"),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order: vec![request_id],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    };
    let folder_path = save_item(&root, &folder_file).expect("the folder writes");
    assert_eq!(folder_path, Path::new(FOLDER_PATH));

    let request_file = Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: request_id,
            name: String::from("Login"),
            method: Method::Post,
            url: String::from("https://api.test/login"),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: api_model::Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections/users-api/sessions"),
        path: None,
    };
    let request_path = save_item(&root, &request_file).expect("the request writes");
    assert_eq!(request_path, Path::new(REQUEST_PATH));

    Fixture {
        _temp: temp,
        root,
        collection_id,
        folder_id,
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
    // A real window is active; a test window is not until asked, and focus events — the
    // rename field's blur among them — only dispatch for an active window.
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
    focus_tree(&tree, cx);
    (workspace, tree, cx)
}

/// Put keyboard focus on the tree, as clicking the panel does.
fn focus_tree(tree: &Entity<CollectionsTree>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus_handle = tree.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
}

/// The rows' names, in draw order.
fn row_names(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        tree.read_with(cx, |tree, _| {
            tree.rows()
                .iter()
                .map(crate::tree::TreeRow::name)
                .map(str::to_owned)
                .collect()
        })
    })
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

/// The items named `name`, as (id, its `order`) — how the tests find a duplicate.
fn items_named(snapshot: &WorkspaceSnapshot, name: &str) -> Vec<(ItemId, Vec<ItemId>)> {
    snapshot
        .items
        .iter()
        .filter(|(_, item)| item.name() == name)
        .map(|(id, _)| (*id, order_of(snapshot, *id)))
        .collect()
}

/// Every container whose `order` claims `child` — what "who claims this file" means
/// (REQ-WSP-01).
fn claimants_of(snapshot: &WorkspaceSnapshot, child: ItemId) -> Vec<ItemId> {
    snapshot
        .items
        .keys()
        .copied()
        .filter(|id| order_of(snapshot, *id).contains(&child))
        .collect()
}

/// Whether the delete confirm modal is the workspace's open modal.
fn confirm_modal_is_open(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<DeleteItemModal>()
                .is_some()
        })
    })
}

/// The platform's new-request keystroke (`ctrl-n` / `cmd-n`).
fn new_request_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-n"
    } else {
        "ctrl-n"
    }
}

/// The platform's duplicate keystroke (`ctrl-d` / `cmd-d`).
fn duplicate_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-d"
    } else {
        "ctrl-d"
    }
}

#[gpui::test]
fn two_quick_creates_both_land_in_the_parent_order(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // Down selects "Users API", and the new-request key fires twice in one breath — two
    // verbs before either write has landed.
    cx.simulate_keystrokes("down");
    draw_window(cx);
    let twice = format!("{} {}", new_request_key(), new_request_key());
    cx.simulate_keystrokes(&twice);
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    let order = order_of(&snapshot, fixture.collection_id);
    assert_eq!(
        order.len(),
        3,
        "the collection's order holds its folder and BOTH created requests"
    );
    assert_eq!(order[0], fixture.folder_id, "the folder keeps its place");
    let (first, second) = (order[1], order[2]);
    assert_ne!(first, second, "the two creates are two items");
    for created in [first, second] {
        assert_ne!(created, fixture.folder_id);
        assert_ne!(created, fixture.request_id);
        assert_eq!(
            snapshot.items.get(&created).map(Item::name).as_deref(),
            Some("New Request"),
            "each create wrote its own request"
        );
    }
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Sessions", "New Request", "New Request"],
        "and both appear in the tree under the expanded collection"
    );
}

#[gpui::test]
fn failed_child_copy_leaves_no_container_claiming_the_original_children(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    // Every slug spelling of the copy's child is taken (REQ-WSP-03 runs out of
    // suffixes), so copying "Login" into "Copy of Sessions" fails part way.
    let copy_dir = fixture.root.join(COPY_DIR);
    std::fs::create_dir_all(&copy_dir).expect("the colliding slug directory writes");
    for attempt in 1..=SLUG_ATTEMPTS {
        let stem = if attempt <= 1 {
            String::from("login")
        } else {
            format!("login-{attempt}")
        };
        let occupant = format!("{{\"id\": \"{}\"}}\n", ItemId::new());
        std::fs::write(copy_dir.join(format!("{stem}.request.json")), occupant)
            .expect("the colliding file writes");
    }
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // Down selects the collection, Right expands it, Down lands on "Sessions".
    cx.simulate_keystrokes("down right down");
    draw_window(cx);
    cx.simulate_keystrokes(duplicate_key());
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    assert!(
        items_named(&snapshot, "Copy of Sessions").is_empty(),
        "the failed duplicate left no copy behind"
    );
    assert_eq!(
        claimants_of(&snapshot, fixture.request_id),
        vec![fixture.folder_id],
        "only the source folder orders its child — the copy never claims it"
    );
    assert_eq!(
        order_of(&snapshot, fixture.folder_id),
        vec![fixture.request_id],
        "and the source subtree is untouched"
    );
    let names = row_names(&tree, cx);
    assert_eq!(
        names.iter().take(2).map(String::as_str).collect::<Vec<_>>(),
        ["Users API", "Sessions"],
        "the tree draws the source subtree"
    );
    assert!(
        !names.iter().any(|name| name == "Copy of Sessions"),
        "and no copy of it"
    );
}

#[gpui::test]
fn duplicating_a_folder_shares_no_ids_and_remaps_every_order(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, _tree, cx) = open_tree(&fixture, cx);

    cx.simulate_keystrokes("down right down");
    draw_window(cx);
    cx.simulate_keystrokes(duplicate_key());
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    let collection_order = order_of(&snapshot, fixture.collection_id);
    assert_eq!(
        collection_order.len(),
        2,
        "the parent's order gains the copy"
    );
    assert_eq!(collection_order[0], fixture.folder_id);
    let copy_id = collection_order[1];
    assert_ne!(copy_id, fixture.folder_id, "the copy is a new artifact");

    let copies = items_named(&snapshot, "Copy of Sessions");
    assert_eq!(
        copies.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![copy_id],
        "the copy is named for what it duplicates"
    );
    let copied_order = copies
        .first()
        .map(|(_, order)| order.clone())
        .unwrap_or_default();
    assert_eq!(
        copied_order.len(),
        1,
        "the copy's order is remapped to the copied children"
    );
    let copied_child = copied_order
        .first()
        .copied()
        .expect("the copy has one child");
    assert_ne!(
        copied_child, fixture.request_id,
        "no id is shared with the source"
    );
    assert_eq!(
        snapshot.items.get(&copied_child).map(Item::name).as_deref(),
        Some("Login"),
        "the child copy keeps its name"
    );
    assert!(
        !copied_order.contains(&fixture.folder_id) && !copied_order.contains(&fixture.request_id),
        "every order is remapped: no id of the source subtree leaks into the copy"
    );
}

#[gpui::test]
fn deleting_a_container_removes_its_subtree_files_and_order_entry(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (workspace, tree, cx) = open_tree(&fixture, cx);

    // Down selects the collection, Right expands it, Down lands on "Sessions".
    cx.simulate_keystrokes("down right down");
    draw_window(cx);
    cx.simulate_keystrokes("delete");
    draw_window(cx);
    assert!(confirm_modal_is_open(&workspace, cx), "Delete asks first");
    let confirm = cx
        .debug_bounds(DELETE_CONFIRM_BUTTON_SELECTOR)
        .expect("the Delete button is on screen");
    cx.simulate_click(confirm.center(), Modifiers::none());
    cx.run_until_parked();

    assert!(
        !fixture.root.join(FOLDER_PATH).exists(),
        "the folder's file is gone"
    );
    assert!(
        !fixture.root.join(REQUEST_PATH).exists(),
        "and every descendant file with it"
    );
    assert!(
        !fixture.root.join(COPY_DIR).exists(),
        "and its slug directory"
    );
    let snapshot = disk_snapshot(&fixture);
    assert!(
        !snapshot.items.contains_key(&fixture.folder_id),
        "the folder is gone from the workspace"
    );
    assert!(
        !snapshot.items.contains_key(&fixture.request_id),
        "and its child with it"
    );
    assert_eq!(
        order_of(&snapshot, fixture.collection_id),
        Vec::<ItemId>::new(),
        "the parent's order no longer names it"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API"],
        "and the tree draws the collection alone"
    );
}

#[gpui::test]
fn rename_field_closes_when_it_loses_focus(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // Down, Right, Down, Right, Down walks the tree onto "Login".
    cx.simulate_keystrokes("down right down right down");
    draw_window(cx);
    cx.simulate_keystrokes("f2");
    draw_window(cx);
    cx.run_until_parked();
    let renaming = cx.read(|cx| {
        tree.read_with(cx, |tree, _| {
            tree.crud.rename_input(fixture.request_id).is_some()
        })
    });
    assert!(renaming, "F2 opens the inline rename field");

    // Focus moves off the field — the user clicked elsewhere in the panel.
    focus_tree(&tree, cx);
    cx.run_until_parked();
    let renaming = cx.read(|cx| {
        tree.read_with(cx, |tree, _| {
            tree.crud.rename_input(fixture.request_id).is_some()
        })
    });
    assert!(
        !renaming,
        "the rename field closes when it loses focus, without writing"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Sessions", "Login"],
        "and the row keeps its old name"
    );
    let snapshot = disk_snapshot(&fixture);
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(REQUEST_PATH)),
        "nothing was written"
    );
}
