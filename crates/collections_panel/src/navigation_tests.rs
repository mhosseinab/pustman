//! The Collections tree's keyboard navigation (S39b): the arrows move the selection
//! through the rows and open and close folders, and Enter opens the selected request in
//! the workspace's center pane — one tab per request, so opening the same request twice
//! focuses the tab it already opened instead of adding a second one (REQ-UI-02).
//!
//! Every assertion is drawn-state (`debug_bounds` on the row's selection hook) or pane
//! state (the tabs the center pane holds), so a keyboard that moves nothing and a tree
//! that draws no selection cannot pass.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, EntityId, Focusable, TestAppContext, VisualTestContext};
use request_editor::RequestEditor;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{ItemHandle, Workspace};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use crate::navigation::init;
use crate::tree::{CollectionsTree, TreeRow};

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

/// The tree the way the shell shows it: a workspace window with the panel in its left
/// dock, the tree focused and the window painted — a keystroke reaches a view only after
/// a paint registered its focus path.
fn open_tree(
    cx: &mut TestAppContext,
    snapshot: WorkspaceSnapshot,
) -> (
    Entity<Workspace>,
    Entity<CollectionsTree>,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    let tree = cx.update(|window, cx| {
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

/// The names of the rows the frame drew with the keyboard selection on them — one row at
/// most, since `debug_bounds` wants a `'static` selector and the row keys are runtime
/// values (the history panel's selector shape).
fn selected_names(tree: &Entity<CollectionsTree>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = cx.read(|cx| tree.read_with(cx, |tree, _| tree.rows().to_vec()));
    rows.iter()
        .filter(|row| {
            let selector: &'static str =
                Box::leak(format!("{}-selected", row.element_id()).into_boxed_str());
            cx.debug_bounds(selector).is_some()
        })
        .map(TreeRow::name)
        .map(str::to_owned)
        .collect()
}

/// What the center pane holds: how many tabs, which one is active, and the request id
/// that tab is keyed by (REQ-UI-02).
struct PaneState {
    tab_count: usize,
    active_index: usize,
    active_id: Option<EntityId>,
    active_request: Option<String>,
}

fn pane_state(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> PaneState {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            let active = pane.active_item();
            PaneState {
                tab_count: pane.item_ids().len(),
                active_index: pane.active_index(),
                active_id: active.map(ItemHandle::entity_id),
                active_request: active
                    .and_then(|item| item.persistent_id(cx))
                    .map(|item_id| item_id.to_string()),
            }
        })
    })
}

/// Whether the active tab's request editor holds keyboard focus — what "opening the tab
/// again focuses it" means for the keyboard.
fn active_editor_is_focused(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let editor = workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
        });
        editor.is_some_and(|editor| {
            let focus_handle = editor.read(cx).focus_handle(cx);
            focus_handle.contains_focused(window, cx)
        })
    })
}

#[gpui::test]
fn keyboard_navigation_expands_and_opens(cx: &mut TestAppContext) {
    let users_id = ItemId::new();
    let sessions_id = ItemId::new();
    let get_user_id = ItemId::new();
    let login_id = ItemId::new();
    let items = vec![
        collection_item(users_id, "Users API", vec![sessions_id, get_user_id]),
        folder_item(sessions_id, "Sessions", vec![login_id]),
        request_item(login_id, "Login", "POST"),
        request_item(get_user_id, "Get user", "GET"),
    ];
    let (workspace, tree, cx) = open_tree(cx, snapshot(items));

    // Collapsed roots, and nothing selected until the keyboard moves.
    assert_eq!(row_names(&tree, cx), vec!["Users API"]);
    assert!(
        selected_names(&tree, cx).is_empty(),
        "a fresh tree has no selection"
    );

    cx.simulate_keystrokes("down");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Users API".to_owned()],
        "Down moves the selection onto the first row"
    );

    // Right expands the selected container and keeps the selection on it.
    cx.simulate_keystrokes("right");
    draw_window(cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Sessions", "Get user"],
        "Right expands the selected collection"
    );
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Users API".to_owned()],
        "expanding keeps the selection on the container"
    );

    cx.simulate_keystrokes("down");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Sessions".to_owned()],
        "Down moves into the folder the parent just revealed"
    );

    cx.simulate_keystrokes("right");
    draw_window(cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Sessions", "Login", "Get user"],
        "Right expands the selected folder"
    );

    cx.simulate_keystrokes("down");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Login".to_owned()],
        "Down moves into the folder's first request"
    );

    // Left on a leaf moves to its parent row.
    cx.simulate_keystrokes("left");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Sessions".to_owned()],
        "Left on a request moves the selection to its parent row"
    );

    // Left on the expanded folder collapses it, selection unmoved.
    cx.simulate_keystrokes("left");
    draw_window(cx);
    assert_eq!(
        row_names(&tree, cx),
        vec!["Users API", "Sessions", "Get user"],
        "Left collapses the expanded folder"
    );
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Sessions".to_owned()],
        "collapsing keeps the selection on the folder"
    );

    // Left on the collapsed folder moves to its parent row.
    cx.simulate_keystrokes("left");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Users API".to_owned()],
        "Left on a collapsed folder moves the selection to its parent row"
    );

    cx.simulate_keystrokes("down down");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Get user".to_owned()],
        "Down walks the revealed rows to the request"
    );

    // Up moves back toward the root.
    cx.simulate_keystrokes("up");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Sessions".to_owned()],
        "Up moves the selection back toward the root"
    );
    cx.simulate_keystrokes("down");
    draw_window(cx);

    // Enter opens the selected request in the center pane.
    press_key(cx, "enter");
    cx.run_until_parked();

    let state = pane_state(&workspace, cx);
    assert_eq!(state.tab_count, 1, "Enter opens the request in one tab");
    assert_eq!(
        state.active_request.as_deref(),
        Some(get_user_id.to_string().as_str()),
        "the tab is keyed by the request's item id"
    );
    assert_eq!(
        state.active_index, 0,
        "the opened tab is the pane's active tab"
    );
}

#[gpui::test]
fn opening_twice_focuses_existing_tab(cx: &mut TestAppContext) {
    let collection_id = ItemId::new();
    let request_id = ItemId::new();
    let items = vec![
        collection_item(collection_id, "Users API", vec![request_id]),
        request_item(request_id, "Get user", "GET"),
    ];
    let (workspace, tree, cx) = open_tree(cx, snapshot(items));

    cx.simulate_keystrokes("down right down");
    draw_window(cx);
    assert_eq!(
        selected_names(&tree, cx),
        vec!["Get user".to_owned()],
        "the arrows land on the request"
    );

    press_key(cx, "enter");
    cx.run_until_parked();

    let first = pane_state(&workspace, cx);
    assert_eq!(first.tab_count, 1, "Enter opens the request in one tab");
    assert_eq!(
        first.active_request.as_deref(),
        Some(request_id.to_string().as_str()),
        "the tab is keyed by the request's item id"
    );

    // Back to the tree, as a user returning to the panel does, and open again.
    focus_tree(&tree, cx);
    press_key(cx, "enter");
    cx.run_until_parked();

    let second = pane_state(&workspace, cx);
    assert_eq!(
        second.tab_count, 1,
        "opening the same request again never adds a second tab"
    );
    assert_eq!(
        second.active_id, first.active_id,
        "opening it again focuses the tab it already opened"
    );
    assert_eq!(
        second.active_request.as_deref(),
        Some(request_id.to_string().as_str()),
        "that tab is still the request's own"
    );
    assert!(
        active_editor_is_focused(&workspace, cx),
        "opening the tab moves keyboard focus into it"
    );
}
