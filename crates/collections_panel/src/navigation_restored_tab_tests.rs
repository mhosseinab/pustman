//! Opening a request whose tab some other opener already put in the center pane — a
//! restored session's, quick open's — focuses that tab instead of adding a second one
//! (#333, REQ-UI-02): the panel matches open tabs by the request's item id, not by the
//! tabs it opened itself.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Auth, Body, Collection, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, Focusable, TestAppContext, VisualTestContext};
use request_editor::RequestEditor;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::{ItemHandle, Workspace};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use crate::navigation::init;
use crate::tree::CollectionsTree;

fn http_request(id: ItemId) -> HttpRequest {
    HttpRequest {
        id,
        name: "Get user".to_owned(),
        method: Method::from_token("GET").expect("GET is a method token"),
        url: "https://api.test/users".to_owned(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn snapshot(collection_id: ItemId, request_id: ItemId) -> WorkspaceSnapshot {
    let items = vec![
        Item {
            body: ItemBody::Collection(Collection {
                id: collection_id,
                name: "Users API".to_owned(),
                description: None,
                auth: Auth::Inherit,
                variables: Vec::new(),
                order: vec![request_id],
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from("collections"),
            path: None,
        },
        Item {
            body: ItemBody::Request(Request::Http(http_request(request_id))),
            dir: PathBuf::from("collections"),
            path: None,
        },
    ];
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

fn focus_tree(tree: &Entity<CollectionsTree>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus_handle = tree.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
}

#[gpui::test]
fn opening_a_request_whose_tab_was_restored_focuses_that_tab(cx: &mut TestAppContext) {
    let collection_id = ItemId::new();
    let request_id = ItemId::new();
    cx.update(init_test);
    cx.update(init);
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();

    // The restored session's tab: opened straight into the pane, the way `Restore`
    // does, before the panel ever saw the request.
    let restored = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(http_request(request_id), window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    // A second, unrelated tab is active, so focusing the restored one is observable.
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.new_tab(cx));
        });
    });

    let tree = cx.update(|window, cx| {
        let tree = cx.new(|cx| CollectionsTree::new(snapshot(collection_id, request_id), cx));
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(tree.clone(), window, cx);
        });
        tree
    });
    focus_tree(&tree, cx);
    cx.simulate_keystrokes("down right down");
    press_key(cx, "enter");
    cx.run_until_parked();

    let (tab_count, active_id) = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            let pane = workspace.center_pane().read(cx);
            (
                pane.item_ids().len(),
                pane.active_item().map(ItemHandle::entity_id),
            )
        })
    });
    assert_eq!(
        tab_count, 2,
        "opening a request that already has a restored tab adds no second tab"
    );
    assert_eq!(
        active_id,
        Some(restored.entity_id()),
        "the restored tab becomes the active one"
    );
    let focused = cx.update(|window, cx| {
        let focus_handle = restored.read(cx).focus_handle(cx);
        focus_handle.contains_focused(window, cx)
    });
    assert!(focused, "the keyboard moves into the restored tab");
}
