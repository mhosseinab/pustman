//! The breadcrumb names where the request lives — its collection, then its folder — and
//! says "Unsaved request" while it has no file; its Copy as cURL button copies the draft.

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{ItemId, Request};
use gpui::{Modifiers, TestAppContext};
use ui::test_support::{draw_window, init_test};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use super::{BREADCRUMB_CURL_SELECTOR, BREADCRUMB_SELECTOR, location_of};
use crate::RequestEditor;

fn container(kind: &str, name: &str, path: &str) -> (ItemId, Item) {
    let id = ItemId::new();
    let value = serde_json::json!({ "id": id, "name": name });
    let body = match kind {
        "collection" => ItemBody::Collection(serde_json::from_value(value).expect("collection")),
        _ => ItemBody::Folder(serde_json::from_value(value).expect("folder")),
    };
    let path = PathBuf::from(path);
    let dir = path
        .parent()
        .and_then(|own| own.parent())
        .map(PathBuf::from)
        .unwrap_or_default();
    (
        id,
        Item {
            body,
            dir,
            path: Some(path),
        },
    )
}

#[test]
fn test_a_saved_request_is_located_under_its_collection_then_its_folder() {
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("List orders");
    let request_id = request.id;
    let mut items = BTreeMap::new();
    for (id, item) in [
        container(
            "folder",
            "Orders",
            "collections/commerce-api/orders/folder.json",
        ),
        container(
            "collection",
            "Commerce API",
            "collections/commerce-api/collection.json",
        ),
        container("collection", "Other", "collections/other/collection.json"),
    ] {
        items.insert(id, item);
    }
    items.insert(
        request_id,
        Item {
            body: ItemBody::Request(Request::Http(request)),
            dir: PathBuf::from("collections/commerce-api/orders"),
            path: Some(PathBuf::from(
                "collections/commerce-api/orders/list-orders.request.json",
            )),
        },
    );
    let snapshot = WorkspaceSnapshot {
        root: PathBuf::from("/workspace"),
        items,
        problems: Vec::new(),
        read_only: false,
    };

    assert_eq!(
        location_of(&snapshot, &request_id),
        Some(vec![String::from("Commerce API"), String::from("Orders")])
    );
    assert_eq!(
        location_of(&snapshot, &ItemId::new()),
        None,
        "a request the workspace does not hold is unsaved"
    );
}

#[gpui::test]
fn test_an_unsaved_request_says_so_and_copies_as_curl(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.test/ping");
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    draw_window(cx);

    assert!(cx.debug_bounds(BREADCRUMB_SELECTOR).is_some());
    assert!(
        editor.read_with(cx, |editor, _| editor.location().is_none()),
        "no workspace holds it, so the breadcrumb reads \"Unsaved request\""
    );

    let button = cx
        .debug_bounds(BREADCRUMB_CURL_SELECTOR)
        .expect("Copy as cURL is on screen");
    cx.simulate_click(button.center(), Modifiers::none());
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default();
    assert!(
        copied.contains("https://example.test/ping"),
        "the draft is on the clipboard as cURL: {copied}"
    );
}
