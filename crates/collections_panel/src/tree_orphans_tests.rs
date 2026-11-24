//! Merge artifacts stay visible in the collections tree (review findings 1 and 2): an
//! item some `order` array claims but no walk can reach — a folder claiming itself, or two
//! folders claiming each other — is still drawn, exactly once, and a collection is always
//! drawn at the root, once, never nested inside another row's subtree.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use super::{CollectionsTree, TreeRow};

/// A request item as a workspace file would parse into.
fn request_item(id: ItemId, name: &str, method: &str) -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id,
            name: name.to_owned(),
            method: Method::from_token(method).expect("the fixture names a real method token"),
            url: "https://api.test/users".to_owned(),
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

/// A folder item holding `order` — the child ids in display order (REQ-WSP-01).
fn folder_item(id: ItemId, name: &str, order: Vec<ItemId>) -> Item {
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

/// A collection item holding `order` — the child ids in display order (REQ-WSP-01).
fn collection_item(id: ItemId, name: &str, order: Vec<ItemId>) -> Item {
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

/// A snapshot over `items`, keyed by each item's own id.
fn snapshot(items: Vec<Item>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root: PathBuf::from("fixtures/workspace"),
        items: items
            .into_iter()
            .map(|item| {
                let id = item.id().expect("every fixture item carries its id");
                (id, item)
            })
            .collect(),
        problems: Vec::new(),
        read_only: false,
    }
}

/// Open the tree the way the shell will: a window with the panel as its root view.
fn open_tree(
    snapshot: WorkspaceSnapshot,
    cx: &mut TestAppContext,
) -> (Entity<CollectionsTree>, &mut VisualTestContext) {
    cx.update(init_test);
    let (tree, cx) = cx.add_window_view(|_window, cx| CollectionsTree::new(snapshot, cx));
    draw_window(cx);
    (tree, cx)
}

/// The rows the tree draws, read out of the panel.
fn tree_rows(tree: &Entity<CollectionsTree>, cx: &VisualTestContext) -> Vec<TreeRow> {
    cx.read(|cx| tree.read_with(cx, |tree, _| tree.rows().to_vec()))
}

/// What each row is, as the tree exposes it: how deep it sits and what it is called.
fn row_shapes(rows: &[TreeRow]) -> Vec<(usize, String)> {
    rows.iter()
        .map(|row| (row.depth(), row.name().to_owned()))
        .collect()
}

/// Every row's stable key — the identity a row draws under.
fn row_keys(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|row| row.element_id().to_string())
        .collect()
}

#[gpui::test]
fn items_claimed_only_by_a_cycle_still_appear_in_the_rows(cx: &mut TestAppContext) {
    let cyclic_id = ItemId::new();
    let inside_id = ItemId::new();
    let left_id = ItemId::new();
    let right_id = ItemId::new();
    let shared_id = ItemId::new();

    // A folder claiming itself, and two folders claiming each other — both shapes a
    // rename/rename merge leaves behind. No collection claims any of them, so only the
    // tree itself can ever draw them.
    let items = vec![
        folder_item(cyclic_id, "Cyclic", vec![cyclic_id, inside_id]),
        folder_item(left_id, "Left", vec![right_id]),
        folder_item(right_id, "Right", vec![left_id, shared_id]),
        request_item(inside_id, "Inside", "GET"),
        request_item(shared_id, "Shared", "POST"),
    ];
    let (tree, cx) = open_tree(snapshot(items), cx);
    tree.update(cx, |tree, cx| {
        tree.set_expanded(cyclic_id, true, cx);
        tree.set_expanded(left_id, true, cx);
        tree.set_expanded(right_id, true, cx);
    });

    let rows = tree_rows(&tree, cx);
    assert_eq!(
        row_shapes(&rows),
        vec![
            (0, "Cyclic".to_owned()),
            (1, "Inside".to_owned()),
            (0, "Left".to_owned()),
            (1, "Right".to_owned()),
            (2, "Shared".to_owned()),
        ],
        "an item only a cycle claims is drawn as an orphan root, with its children"
    );

    let keys = row_keys(&rows);
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(keys.len(), unique.len(), "no row is drawn twice: {keys:?}");
}

#[gpui::test]
fn a_collection_claimed_by_a_folder_draws_once_at_the_root(cx: &mut TestAppContext) {
    let alpha_id = ItemId::new();
    let beta_id = ItemId::new();
    let first_id = ItemId::new();
    let second_id = ItemId::new();

    // `Beta` is claimed inside `Alpha`'s order — a merge artifact that must not nest a
    // collection under another row.
    let items = vec![
        collection_item(alpha_id, "Alpha", vec![beta_id, first_id]),
        collection_item(beta_id, "Beta", vec![second_id]),
        request_item(first_id, "First", "GET"),
        request_item(second_id, "Second", "POST"),
    ];
    let (tree, cx) = open_tree(snapshot(items), cx);
    tree.update(cx, |tree, cx| {
        tree.set_expanded(alpha_id, true, cx);
        tree.set_expanded(beta_id, true, cx);
    });

    let rows = tree_rows(&tree, cx);
    assert_eq!(
        row_shapes(&rows),
        vec![
            (0, "Alpha".to_owned()),
            (1, "First".to_owned()),
            (0, "Beta".to_owned()),
            (1, "Second".to_owned()),
        ],
        "a collection is drawn once at the root, never nested inside another row"
    );

    let keys = row_keys(&rows);
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(keys.len(), unique.len(), "no row is drawn twice: {keys:?}");
}
