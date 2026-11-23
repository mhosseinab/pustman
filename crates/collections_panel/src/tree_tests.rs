//! The collections tree draws the snapshot's rows the way the snapshot orders them:
//! children follow their parent's `order` array at every depth (never the name order or
//! the directory order), a collapsed container hides its children, and a file the
//! snapshot could not read shows up as a row with its error badge. The rows are
//! virtualized: a 10,000-request tree holds every row in its model and draws only the
//! rows its window can show (REQ-NFR-02).

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};
use workspace_store::{Item, ItemBody, ItemProblem, ItemProblemKind, WorkspaceSnapshot};

use super::{CollectionsTree, TreeRow};

/// The request count the virtualization test puts under one collection — far more than
/// any window can show.
const HUGE_TREE_REQUESTS: usize = 10_000;

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
fn snapshot(items: Vec<Item>, problems: Vec<ItemProblem>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root: PathBuf::from("fixtures/workspace"),
        items: items
            .into_iter()
            .map(|item| {
                let id = item.id().expect("every fixture item carries its id");
                (id, item)
            })
            .collect(),
        problems,
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

/// The method badge text of the request rows, in draw order.
fn method_badges(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| row.method().map(str::to_owned))
        .collect()
}

/// Whether the frame drew `row`'s element. `debug_bounds` wants a `'static` selector,
/// so the row's runtime-built key is leaked — a few bytes per test (the history panel's
/// click helper does the same).
fn row_is_drawn(row: &TreeRow, cx: &mut VisualTestContext) -> bool {
    let selector: &'static str = Box::leak(row.element_id().to_string().into_boxed_str());
    cx.debug_bounds(selector).is_some()
}

/// The test hook on a problem row's error badge: the row's stable key plus `badge` (the
/// `history` panel's delete-button selector shape).
fn badge_selector(row: &TreeRow) -> String {
    format!("{}-badge", row.element_id())
}

#[gpui::test]
fn tree_follows_order_arrays(cx: &mut TestAppContext) {
    let admin_id = ItemId::new();
    let users_id = ItemId::new();
    let sessions_id = ItemId::new();
    let get_user_id = ItemId::new();
    let login_id = ItemId::new();
    let delete_user_id = ItemId::new();
    let deleted_on_disk = ItemId::new();

    let items = vec![
        // The `order` arrays are deliberately neither the name order nor the order the
        // items are inserted in, so a tree that guesses either one fails this test.
        collection_item(
            users_id,
            "Users API",
            vec![delete_user_id, sessions_id, get_user_id],
        ),
        collection_item(admin_id, "Admin", Vec::new()),
        folder_item(sessions_id, "Sessions", vec![login_id, deleted_on_disk]),
        request_item(get_user_id, "Get user", "GET"),
        request_item(login_id, "Login", "POST"),
        request_item(delete_user_id, "Delete user", "DELETE"),
    ];
    let problems = vec![ItemProblem {
        path: PathBuf::from("collections/broken/collection.json"),
        kind: ItemProblemKind::ConflictMarkers,
    }];
    let (tree, cx) = open_tree(snapshot(items, problems), cx);

    // Collapsed: the roots only — the collections and the unreadable file, nothing
    // nested under them.
    let collapsed = tree_rows(&tree, cx);
    assert_eq!(
        row_shapes(&collapsed),
        vec![
            (0, "Admin".to_owned()),
            (0, "Users API".to_owned()),
            (0, "collections/broken/collection.json".to_owned()),
        ],
        "a collapsed container holds its children back"
    );

    tree.update(cx, |tree, cx| {
        tree.set_expanded(users_id, true, cx);
        tree.set_expanded(sessions_id, true, cx);
    });

    let expanded = tree_rows(&tree, cx);
    assert_eq!(
        row_shapes(&expanded),
        vec![
            (0, "Admin".to_owned()),
            (0, "Users API".to_owned()),
            (1, "Delete user".to_owned()),
            (1, "Sessions".to_owned()),
            (2, "Login".to_owned()),
            (1, "Get user".to_owned()),
            (0, "collections/broken/collection.json".to_owned()),
        ],
        "children follow the parent's `order` array at every depth; the id whose file is\
         \n gone (`Sessions`' second child) is skipped, never guessed at"
    );
    assert_eq!(
        method_badges(&expanded),
        vec!["DELETE".to_owned(), "POST".to_owned(), "GET".to_owned()],
        "every request row carries its method"
    );

    // The unreadable file draws its error badge at the end of the tree.
    draw_window(cx);
    let rows = tree_rows(&tree, cx);
    let problem = rows
        .last()
        .expect("the unreadable file is the tree's last row");
    let selector: &'static str = Box::leak(badge_selector(problem).into_boxed_str());
    assert!(
        cx.debug_bounds(selector).is_some(),
        "a file the snapshot could not read shows its error badge"
    );
}

#[gpui::test]
fn ten_thousand_items_render_only_visible_rows(cx: &mut TestAppContext) {
    let collection_id = ItemId::new();
    let mut order = Vec::with_capacity(HUGE_TREE_REQUESTS);
    let mut items = Vec::with_capacity(HUGE_TREE_REQUESTS + 1);
    for index in 0..HUGE_TREE_REQUESTS {
        let id = ItemId::new();
        order.push(id);
        items.push(request_item(id, &format!("Request {index}"), "GET"));
    }
    items.push(collection_item(collection_id, "Bulk", order));

    let (tree, cx) = open_tree(snapshot(items, Vec::new()), cx);
    tree.update(cx, |tree, cx| tree.set_expanded(collection_id, true, cx));
    draw_window(cx);

    let rows = tree_rows(&tree, cx);
    assert_eq!(
        rows.len(),
        HUGE_TREE_REQUESTS + 1,
        "the tree holds every row in its model"
    );

    let drawn: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row_is_drawn(row, cx))
        .map(|(index, _)| index)
        .collect();
    assert!(
        (2..=128).contains(&drawn.len()),
        "only the rows the window can show may render; {} of {} did",
        drawn.len(),
        rows.len()
    );
    assert_eq!(
        drawn.first(),
        Some(&0),
        "the top row is on screen and rendered"
    );
    assert_eq!(
        drawn.last().map(|last| last + 1),
        Some(drawn.len()),
        "the rendered rows are one contiguous window from the top"
    );
    assert!(
        !row_is_drawn(&rows[5000], cx),
        "a row 5000 down is far outside the viewport and must not render at all"
    );
    assert!(
        !row_is_drawn(&rows[HUGE_TREE_REQUESTS], cx),
        "the last row is far outside the viewport and must not render at all"
    );
}
