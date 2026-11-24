//! Collection CRUD (S40a), driven the way the shell drives it: a real workspace folder
//! on disk (the three promises are about files), a workspace window with the panel in
//! its left dock, and the keyboard plus the confirm modal the user touches.
//!
//! Each test walks one promise end to end — the create writes the file and the tree
//! draws it, the rename keeps the id and moves the file to the new slug, the delete
//! removes the file only behind a confirmed dialog — so a write that skips
//! `workspace_store`, a rename that re-identifies, or a delete without a confirm fails
//! the test that names it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{Auth, Body, Collection, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use super::{DELETE_CANCEL_BUTTON_SELECTOR, DELETE_CONFIRM_BUTTON_SELECTOR, DeleteItemModal};
use crate::tree::{CollectionsTree, TreeRow};

/// The collection's file, as the fixture writes it: slug of "Users API" (REQ-WSP-03).
const COLLECTION_PATH: &str = "collections/users-api/collection.json";

/// The request's file, as the fixture writes it: slug of "Get user".
const REQUEST_PATH: &str = "collections/users-api/get-user.request.json";

/// The request's file once the rename names it "Fetch all users".
const RENAMED_REQUEST_PATH: &str = "collections/users-api/fetch-all-users.request.json";

/// The file the create writes for its "New Request".
const NEW_REQUEST_PATH: &str = "collections/users-api/new-request.request.json";

/// The workspace folder under test, and the stable ids the fixture seeded it with.
struct Fixture {
    /// Held so the folder outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    request_id: ItemId,
}

/// A workspace on disk — `pustman.json`, one collection "Users API" ordering one
/// request "Get user" — written through `workspace_store::save_item`, the same door the
/// CRUD operations use (REQ-WSP-01).
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    let request_id = ItemId::new();

    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("CRUD fixture"),
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
            order: vec![request_id],
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

    let request_file = Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: request_id,
            name: String::from("Get user"),
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
    let request_path = save_item(&root, &request_file).expect("the request writes");
    assert_eq!(
        request_path,
        Path::new(REQUEST_PATH),
        "the fixture sits where its slugs say it does"
    );

    Fixture {
        _temp: temp,
        root,
        collection_id,
        request_id,
    }
}

/// The tree the way the shell shows it: a workspace window with the panel in its left
/// dock, every key binding this step adds wired, and the tree focused — a keystroke
/// reaches a handler only after a paint registered its focus path.
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
        // Trap: without `text_input::init` the inline rename field's Enter is unbound
        // and the commit never fires; without `navigation::init` the arrows cannot put
        // the selection on the row the keystroke names.
        text_input::init(cx);
        crate::navigation::init(cx);
        crate::crud::init(cx);
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

/// The ids the collection's `order` holds (REQ-WSP-01).
fn collection_order(snapshot: &WorkspaceSnapshot, collection_id: ItemId) -> Vec<ItemId> {
    match snapshot.items.get(&collection_id).map(|item| &item.body) {
        Some(ItemBody::Collection(collection)) => collection.order.clone(),
        _ => Vec::new(),
    }
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

/// The platform's new-request keystroke (`ctrl-n` / `cmd-n`, the two platforms' own
/// "new" modifiers — `.agents/rules/keymaps.md`'s split).
fn new_request_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-n"
    } else {
        "ctrl-n"
    }
}

/// The platform's select-all keystroke (`ctrl-a` / `cmd-a`).
fn select_all_keys() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[gpui::test]
fn create_request_writes_file_and_appears_in_tree(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API"],
        "the fixture starts with one collapsed collection"
    );

    // The collection is the create's target: Down selects it and the new-request key
    // creates inside it (the file-explorer convention).
    cx.simulate_keystrokes("down");
    draw_window(cx);
    cx.simulate_keystrokes(new_request_key());
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    let order = collection_order(&snapshot, fixture.collection_id);
    assert_eq!(
        order.len(),
        2,
        "the collection's order grew by the new request"
    );
    assert_eq!(
        order[0], fixture.request_id,
        "the old child keeps its place"
    );
    let created = order[1];
    assert_ne!(
        created, fixture.request_id,
        "the new request has its own id"
    );
    assert_eq!(
        snapshot.items.get(&created).map(Item::name).as_deref(),
        Some("New Request"),
        "the new request is named for what it is"
    );
    assert_eq!(
        snapshot.path(&created),
        Some(Path::new(NEW_REQUEST_PATH)),
        "the file is written under the collection's slug directory (REQ-WSP-01/03)"
    );
    assert!(
        fixture.root.join(NEW_REQUEST_PATH).exists(),
        "and it is on disk, not only in the snapshot"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Get user", "New Request"],
        "the new request appears in the tree, under its just-expanded collection"
    );
}

#[gpui::test]
fn rename_keeps_id_and_updates_slug(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (_workspace, tree, cx) = open_tree(&fixture, cx);

    // Down selects the collection, Right expands it, Down lands on "Get user".
    cx.simulate_keystrokes("down right down");
    draw_window(cx);
    assert_eq!(row_names(&tree, cx), vec!["Users API", "Get user"]);

    // F2 renames inline: the row's name becomes a field holding the old name, so
    // select-all plus typing replaces it and Enter commits. The select-all is the
    // platform keystroke — the field preselects the name itself on the next frame
    // (`on_next_frame`), which a test window never pumps (`Decision:` below).
    cx.simulate_keystrokes("f2");
    draw_window(cx);
    cx.run_until_parked();
    cx.simulate_keystrokes(select_all_keys());
    cx.simulate_input("Fetch all users");
    press_key(cx, "enter");
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    let renamed = snapshot
        .items
        .get(&fixture.request_id)
        .expect("the request is still the same item");
    assert_eq!(
        renamed.id(),
        Some(fixture.request_id),
        "the rename keeps the id (REQ-WSP-03)"
    );
    assert_eq!(renamed.name(), "Fetch all users");
    assert_eq!(
        snapshot.path(&fixture.request_id),
        Some(Path::new(RENAMED_REQUEST_PATH)),
        "the file name follows the new slug"
    );
    assert!(
        !fixture.root.join(REQUEST_PATH).exists(),
        "the old slug file is gone — the rename moved it"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Fetch all users"],
        "the tree draws the new name"
    );
}

#[gpui::test]
fn delete_removes_file_after_confirm(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (workspace, tree, cx) = open_tree(&fixture, cx);
    cx.simulate_keystrokes("down right down");
    draw_window(cx);

    // A delete that is not confirmed removes nothing.
    cx.simulate_keystrokes("delete");
    draw_window(cx);
    assert!(
        confirm_modal_is_open(&workspace, cx),
        "Delete opens the confirm modal before anything is removed"
    );
    let cancel = cx
        .debug_bounds(DELETE_CANCEL_BUTTON_SELECTOR)
        .expect("the Cancel button is on screen while the modal is open");
    cx.simulate_click(cancel.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        fixture.root.join(REQUEST_PATH).exists(),
        "a cancelled confirm removes nothing"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Get user"],
        "and the tree is left as it was"
    );

    // The confirmed delete removes the file, the order entry and the row.
    cx.simulate_keystrokes("delete");
    draw_window(cx);
    let confirm = cx
        .debug_bounds(DELETE_CONFIRM_BUTTON_SELECTOR)
        .expect("the Delete button is on screen while the modal is open");
    cx.simulate_click(confirm.center(), Modifiers::none());
    cx.run_until_parked();

    let snapshot = disk_snapshot(&fixture);
    assert!(
        !fixture.root.join(REQUEST_PATH).exists(),
        "the confirmed delete removes the file"
    );
    assert!(
        !snapshot.items.contains_key(&fixture.request_id),
        "the request is gone from the workspace"
    );
    assert!(
        collection_order(&snapshot, fixture.collection_id).is_empty(),
        "the collection no longer orders it"
    );
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API"],
        "and the tree no longer draws it"
    );
}
