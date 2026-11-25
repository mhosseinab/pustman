//! The breadcrumb in a real workspace window: an editor opened over a workspace that
//! holds its request shows `Collection / Folder`, and the trail follows a snapshot
//! change that renames the folder.

use std::path::{Path, PathBuf};

use api_model::{HttpRequest, ItemId, Request};
use gpui::{Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::prelude::*;
use ui::test_support::init_test;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item as StoreItem, ItemBody, WorkspaceSnapshot};

use crate::RequestEditor;

fn save(root: &Path, body: ItemBody, dir: &str) {
    let item = StoreItem {
        body,
        dir: PathBuf::from(dir),
        path: None,
    };
    workspace_store::save_item(root, &item).expect("the fixture item saves");
}

/// `pustman.json`, a collection "Commerce API" holding folder "Orders" holding `request`.
fn fixture(root: &Path, request: &HttpRequest) {
    let workspace = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Breadcrumb workspace",
        "format_version": "1.0",
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&workspace).expect("serializes"),
    )
    .expect("the workspace file is written");
    let folder_id = ItemId::new();
    let collection = serde_json::json!({
        "id": ItemId::new(), "name": "Commerce API", "order": [folder_id],
    });
    let folder = serde_json::json!({ "id": folder_id, "name": "Orders", "order": [request.id] });
    save(
        root,
        ItemBody::Collection(serde_json::from_value(collection).expect("collection")),
        "collections",
    );
    save(
        root,
        ItemBody::Folder(serde_json::from_value(folder).expect("folder")),
        "collections/commerce-api",
    );
    save(
        root,
        ItemBody::Request(Request::Http(request.clone())),
        "collections/commerce-api/orders",
    );
}

fn open(
    cx: &mut TestAppContext,
    root: PathBuf,
    request: HttpRequest,
) -> (
    Entity<RequestEditor>,
    Entity<WorkspaceOpen>,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    cx.update(|cx| {
        workspace::init(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    let opener = cx.update(|_, cx| {
        WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen")
    });
    opener.update(cx, |opener, cx| opener.open_folder(root, cx));
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    cx.run_until_parked();
    (editor, opener, cx)
}

#[gpui::test]
fn test_an_editor_in_a_workspace_shows_its_trail_and_follows_a_folder_rename(
    cx: &mut TestAppContext,
) {
    let folder = TempDir::new().expect("a temp folder");
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("List orders");
    fixture(folder.path(), &request);
    let (editor, opener, cx) = open(cx, folder.path().to_path_buf(), request);

    let trail = editor.read_with(cx, |editor, _| editor.location().map(<[String]>::to_vec));
    assert_eq!(
        trail,
        Some(vec![String::from("Commerce API"), String::from("Orders")]),
        "the breadcrumb names the request's collection, then its folder"
    );

    opener.update(cx, |opener, cx| {
        let open = opener.snapshot().expect("the workspace is open");
        let mut snapshot = WorkspaceSnapshot {
            root: open.root.clone(),
            items: open.items.clone(),
            problems: Vec::new(),
            read_only: open.read_only,
        };
        for item in snapshot.items.values_mut() {
            if matches!(item.body, ItemBody::Folder(_)) {
                item.set_name("Purchases");
            }
        }
        opener.set_snapshot(snapshot, cx);
    });
    cx.run_until_parked();

    let trail = editor.read_with(cx, |editor, _| editor.location().map(<[String]>::to_vec));
    assert_eq!(
        trail,
        Some(vec![
            String::from("Commerce API"),
            String::from("Purchases")
        ]),
        "the trail follows the renamed folder"
    );
}
