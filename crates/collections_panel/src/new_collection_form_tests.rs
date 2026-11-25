//! The New Collection form driven the way a user drives it: `NewCollection` in a window
//! with a workspace open opens the form, Create writes the collection through the tree's
//! write queue and selects it; with no workspace open the action offers New Workspace… and
//! Open Workspace… instead of doing nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Collection, ItemId};
use gpui::{AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::prelude::FormField;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace::new_workspace_form::NewWorkspaceForm;
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use super::{
    CREATE_BUTTON_SELECTOR, NAME_FIELD, NEW_WORKSPACE_BUTTON_SELECTOR, NewCollectionForm,
    NoWorkspaceModal, OPEN_WORKSPACE_BUTTON_SELECTOR,
};
use crate::crud::NewCollection;
use crate::tree::CollectionsTree;

/// A workspace on disk with one collection, "Users API".
fn seed_workspace() -> TempDir {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path();
    let workspace_file = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("Form fixture"),
            format_version: String::from(workspace_store::FORMAT_VERSION),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    };
    save_item(root, &workspace_file).expect("the workspace file writes");
    let collection = Item {
        body: ItemBody::Collection(Collection {
            id: ItemId::new(),
            name: String::from("Users API"),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order: Vec::new(),
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    };
    save_item(root, &collection).expect("the collection writes");
    temp
}

/// A workspace window with the panel in its left dock showing `snapshot`, the app-level
/// wiring this crate and `workspace` install, and the tree focused.
fn open_tree(
    snapshot: WorkspaceSnapshot,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<CollectionsTree>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        workspace::init(cx);
        crate::navigation::init(cx);
        crate::crud::init(cx);
        crate::new_collection_form::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    let tree = cx.update(|window, cx| {
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

fn empty_snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root: PathBuf::new(),
        items: BTreeMap::new(),
        problems: Vec::new(),
        read_only: false,
    }
}

fn active<V: workspace::ModalView>(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Option<Entity<V>> {
    cx.read(|cx| workspace.read(cx).modal_layer().active_modal::<V>())
}

fn fill(
    form: &Entity<NewCollectionForm>,
    name: &str,
    description: &str,
    cx: &mut VisualTestContext,
) {
    cx.update(|_window, cx| {
        let (name_input, description_input) = form.read(cx).fields();
        name_input.update(cx, |input, cx| input.set_text(name, cx));
        description_input.update(cx, |input, cx| input.set_text(description, cx));
    });
    cx.run_until_parked();
    draw_window(cx);
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The selected row's item name, if any.
fn selected_name(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Option<String> {
    cx.read(|cx| {
        let tree = tree.read(cx);
        let id = tree.selected_item()?;
        tree.snapshot.items.get(&id).map(Item::name)
    })
}

#[gpui::test]
fn new_collection_form_creates_and_selects(cx: &mut TestAppContext) {
    let folder = seed_workspace();
    let snapshot = workspace_store::open(folder.path()).expect("the seeded workspace opens");
    let (workspace, tree, cx) = open_tree(snapshot, cx);

    cx.dispatch_action(NewCollection);
    cx.run_until_parked();
    let form = active::<NewCollectionForm>(&workspace, cx)
        .expect("NewCollection opens the New Collection form");

    fill(&form, "Orders", "Order endpoints", cx);
    click(CREATE_BUTTON_SELECTOR, cx);

    let written = workspace_store::open(folder.path()).expect("the workspace still opens");
    let orders = written
        .items
        .values()
        .find(|item| item.name() == "Orders")
        .expect("Create writes the collection to the workspace");
    match &orders.body {
        ItemBody::Collection(collection) => assert_eq!(
            collection.description.as_deref(),
            Some("Order endpoints"),
            "the description is saved with it"
        ),
        other => panic!("the new item is a collection, got {other:?}"),
    }
    assert!(
        folder
            .path()
            .join("collections/orders/collection.json")
            .is_file(),
        "the collection file sits at collections/<slug>/collection.json"
    );
    assert!(
        active::<NewCollectionForm>(&workspace, cx).is_none(),
        "the form closes on Create"
    );
    assert_eq!(
        selected_name(&tree, cx).as_deref(),
        Some("Orders"),
        "the new collection is selected in the tree"
    );
}

#[gpui::test]
fn new_collection_form_numbers_a_name_a_sibling_already_has(cx: &mut TestAppContext) {
    let folder = seed_workspace();
    let snapshot = workspace_store::open(folder.path()).expect("the seeded workspace opens");
    let (workspace, _tree, cx) = open_tree(snapshot, cx);
    cx.dispatch_action(NewCollection);
    cx.run_until_parked();
    let form = active::<NewCollectionForm>(&workspace, cx).expect("the form opens");

    fill(&form, "Users API", "", cx);
    assert!(
        cx.debug_bounds(String::leak(FormField::hint_selector(NAME_FIELD)))
            .is_some(),
        "the form says the name is taken and what it will be saved as"
    );
    assert_eq!(cx.read(|cx| form.read(cx).saved_name(cx)), "Users API (2)");

    fill(&form, "", "", cx);
    assert!(
        cx.debug_bounds(String::leak(FormField::error_selector(NAME_FIELD)))
            .is_some(),
        "an empty name shows its error inline"
    );
    assert!(!cx.read(|cx| form.read(cx).can_create(cx)));

    fill(&form, "Users API", "", cx);
    click(CREATE_BUTTON_SELECTOR, cx);
    let written = workspace_store::open(folder.path()).expect("the workspace still opens");
    assert!(
        written
            .items
            .values()
            .any(|item| item.name() == "Users API (2)"),
        "the collection is saved under the numbered name the form showed"
    );
}

#[gpui::test]
fn new_collection_without_workspace_offers_workspace_actions(cx: &mut TestAppContext) {
    let (workspace, _tree, cx) = open_tree(empty_snapshot(), cx);

    cx.dispatch_action(NewCollection);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        active::<NoWorkspaceModal>(&workspace, cx).is_some(),
        "with no workspace open, NewCollection explains why instead of doing nothing"
    );
    assert!(
        cx.debug_bounds(OPEN_WORKSPACE_BUTTON_SELECTOR).is_some(),
        "it offers Open workspace…"
    );

    click(NEW_WORKSPACE_BUTTON_SELECTOR, cx);
    assert!(
        active::<NoWorkspaceModal>(&workspace, cx).is_none(),
        "the dialog hands over"
    );
    assert!(
        active::<NewWorkspaceForm>(&workspace, cx).is_some(),
        "New workspace… opens the New Workspace form"
    );
}

#[gpui::test]
fn two_queued_creates_with_one_name_never_make_two_same_named_collections(cx: &mut TestAppContext) {
    let folder = seed_workspace();
    let snapshot = workspace_store::open(folder.path()).expect("the seeded workspace opens");
    let (_workspace, tree, cx) = open_tree(snapshot, cx);

    // Two forms submitted back to back: both saw the same snapshot, neither write has
    // landed when the second is queued.
    cx.update(|window, cx| {
        tree.update(cx, |tree, cx| {
            tree.create_collection(String::from("Orders"), None, window, cx);
            tree.create_collection(String::from("Orders"), None, window, cx);
        });
    });
    cx.run_until_parked();

    let written = workspace_store::open(folder.path()).expect("the workspace still opens");
    let mut names: Vec<String> = written
        .items
        .values()
        .filter(|item| matches!(item.body, ItemBody::Collection(_)))
        .map(Item::name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["Orders", "Orders (2)", "Users API"],
        "the second create is numbered against what the first wrote"
    );
}
