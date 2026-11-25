//! The Collections sidebar's own controls (S48u3): the empty states with real buttons —
//! no workspace (New workspace… / Open workspace…) and a workspace with no collections
//! (New collection… / Import…) — the "＋ New" beside "Find in collections", the name
//! filter that keeps a match's containers, and the selected row's accent bar.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, Window};
use pustman_actions::import_export::Import;
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use super::{
    CollectionsTree, EMPTY_IMPORT_ID, EMPTY_NEW_COLLECTION_ID, EMPTY_NEW_WORKSPACE_ID,
    EMPTY_OPEN_WORKSPACE_ID, NEW_COLLECTION_BUTTON_ID, NO_COLLECTIONS_ID, NO_MATCHES_ID,
    NO_WORKSPACE_ID,
};
use crate::crud::NewCollection;

/// What the host saw pass through it: every [`NewCollection`] dispatched into the tree.
struct Host {
    tree: Entity<CollectionsTree>,
    new_collections: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let new_collections = self.new_collections.clone();
        div()
            .size_full()
            // Capture phase: counts the action on its way down without consuming it.
            .capture_action(move |_: &NewCollection, _window, _cx| {
                new_collections.set(new_collections.get() + 1);
            })
            .child(self.tree.clone())
    }
}

fn http_request(id: ItemId, name: &str) -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id,
            name: name.to_owned(),
            method: Method::from_token("GET").expect("GET is a method"),
            url: "https://api.test/orders".to_owned(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections"),
        path: None,
    }
}

fn folder(id: ItemId, name: &str, order: Vec<ItemId>) -> Item {
    Item {
        body: ItemBody::Folder(Folder {
            id,
            name: name.to_owned(),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order,
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    }
}

fn collection(id: ItemId, name: &str, order: Vec<ItemId>) -> Item {
    Item {
        body: ItemBody::Collection(Collection {
            id,
            name: name.to_owned(),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order,
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    }
}

fn snapshot(root: PathBuf, items: Vec<Item>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root,
        items: items
            .into_iter()
            .map(|item| (item.id().expect("fixture items carry ids"), item))
            .collect(),
        problems: Vec::new(),
        read_only: false,
    }
}

/// The tree with its filter, inside a [`Host`], with app-level counters for the
/// workspace and import actions.
struct Opened<'a> {
    tree: Entity<CollectionsTree>,
    new_collections: Rc<Cell<usize>>,
    created: Rc<Cell<usize>>,
    opened: Rc<Cell<usize>>,
    imported: Rc<Cell<usize>>,
    cx: &'a mut VisualTestContext,
}

fn open(snapshot: WorkspaceSnapshot, cx: &mut TestAppContext) -> Opened<'_> {
    cx.update(init_test);
    cx.update(text_input::init);
    let created = Rc::new(Cell::new(0));
    let opened = Rc::new(Cell::new(0));
    let imported = Rc::new(Cell::new(0));
    cx.update(|cx| {
        let created = created.clone();
        cx.on_action(move |_: &CreateWorkspace, _cx| created.set(created.get() + 1));
        let opened = opened.clone();
        cx.on_action(move |_: &OpenWorkspace, _cx| opened.set(opened.get() + 1));
        let imported = imported.clone();
        cx.on_action(move |_: &Import, _cx| imported.set(imported.get() + 1));
    });
    let new_collections = Rc::new(Cell::new(0));
    let (host, cx) = cx.add_window_view({
        let new_collections = new_collections.clone();
        move |window, cx| {
            let tree = cx.new(|cx| CollectionsTree::new_in_window(snapshot, window, cx));
            Host {
                tree,
                new_collections,
            }
        }
    });
    let tree = cx.read(|cx| host.read(cx).tree.clone());
    draw_window(cx);
    Opened {
        tree,
        new_collections,
        created,
        opened,
        imported,
        cx,
    }
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn row_names(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        tree.read(cx)
            .rows()
            .iter()
            .map(|row| row.name().to_owned())
            .collect()
    })
}

#[gpui::test]
fn test_without_a_workspace_the_sidebar_offers_new_and_open_workspace(cx: &mut TestAppContext) {
    let opened = open(snapshot(PathBuf::new(), Vec::new()), cx);
    let cx = opened.cx;
    assert!(cx.debug_bounds(NO_WORKSPACE_ID).is_some(), "the no-workspace state renders");
    assert!(
        cx.debug_bounds(NEW_COLLECTION_BUTTON_ID).is_none(),
        "no ＋ New while there is no workspace to put a collection in"
    );
    click(cx, EMPTY_NEW_WORKSPACE_ID);
    assert_eq!(opened.created.get(), 1, "New workspace… dispatches CreateWorkspace");
    click(cx, EMPTY_OPEN_WORKSPACE_ID);
    assert_eq!(opened.opened.get(), 1, "Open workspace… dispatches OpenWorkspace");
}

#[gpui::test]
fn test_an_empty_workspace_offers_new_collection_and_import(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir is created");
    let opened = open(snapshot(directory.path().to_path_buf(), Vec::new()), cx);
    let cx = opened.cx;
    assert!(cx.debug_bounds(NO_COLLECTIONS_ID).is_some(), "the empty-workspace state renders");
    click(cx, EMPTY_NEW_COLLECTION_ID);
    assert_eq!(
        opened.new_collections.get(),
        1,
        "New collection… dispatches NewCollection into the tree"
    );
    // The create may have landed a collection by now; the Import button belongs to the
    // empty state, so press it on a fresh empty tree.
    let directory = tempfile::tempdir().expect("the temp dir is created");
    opened.tree.update(cx, |tree, cx| {
        tree.set_snapshot(snapshot(directory.path().to_path_buf(), Vec::new()), cx);
    });
    draw_window(cx);
    click(cx, EMPTY_IMPORT_ID);
    assert_eq!(opened.imported.get(), 1, "Import… dispatches Import");
}

#[gpui::test]
fn test_the_new_button_beside_the_filter_dispatches_new_collection(cx: &mut TestAppContext) {
    let collection_id = ItemId::new();
    let opened = open(
        snapshot(
            PathBuf::from("fixtures/workspace"),
            vec![collection(collection_id, "Commerce API", Vec::new())],
        ),
        cx,
    );
    let cx = opened.cx;
    assert!(cx.debug_bounds("collections-filter").is_some(), "the filter field renders");
    click(cx, NEW_COLLECTION_BUTTON_ID);
    assert_eq!(
        opened.new_collections.get(),
        1,
        "＋ New dispatches NewCollection into the tree"
    );
}

#[gpui::test]
fn test_the_filter_keeps_matching_rows_and_their_containers(cx: &mut TestAppContext) {
    let (commerce, orders, list, health) =
        (ItemId::new(), ItemId::new(), ItemId::new(), ItemId::new());
    let opened = open(
        snapshot(
            PathBuf::from("fixtures/workspace"),
            vec![
                collection(commerce, "Commerce API", vec![orders, health]),
                folder(orders, "Orders", vec![list]),
                http_request(list, "List orders"),
                http_request(health, "Health check"),
            ],
        ),
        cx,
    );
    let (tree, cx) = (opened.tree, opened.cx);
    assert_eq!(row_names(&tree, cx), vec!["Commerce API"], "the tree starts collapsed");

    let filter = cx.read(|cx| tree.read(cx).filter().cloned()).expect("the tree has a filter");
    filter.update(cx, |filter, cx| filter.set_text("LIST", cx));
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Commerce API", "Orders", "List orders"],
        "a match shows with the containers above it, through collapsed ones"
    );

    filter.update(cx, |filter, cx| filter.set_text("nothing like it", cx));
    cx.run_until_parked();
    draw_window(cx);
    assert!(cx.debug_bounds(NO_MATCHES_ID).is_some(), "an empty result says so");

    filter.update(cx, |filter, cx| filter.set_text("", cx));
    cx.run_until_parked();
    assert_eq!(
        row_names(&tree, cx),
        vec!["Commerce API"],
        "clearing the filter brings back the tree as the user left it"
    );
}

#[gpui::test]
fn test_the_selected_row_carries_the_accent_bar(cx: &mut TestAppContext) {
    let collection_id = ItemId::new();
    let opened = open(
        snapshot(
            PathBuf::from("fixtures/workspace"),
            vec![collection(collection_id, "Commerce API", Vec::new())],
        ),
        cx,
    );
    let cx = opened.cx;
    let accent: &'static str = format!("tree-item-{collection_id}-accent").leak();
    assert!(cx.debug_bounds(accent).is_none(), "no row is selected yet");
    click(cx, format!("tree-item-{collection_id}-name").leak());
    let bar = cx.debug_bounds(accent).expect("the selected row draws its accent bar");
    assert!(
        bar.size.width <= px(4.) && bar.size.height >= px(20.),
        "the bar is a thin full-height marker, got {bar:?}"
    );
}
