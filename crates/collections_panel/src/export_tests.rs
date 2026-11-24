//! Exporting a collection as Postman v2.1 (S45c, REQ-IO-03), driven the way the app
//! drives it: the handler the row menu's Export calls, the platform save dialog answered
//! with the destination, the background write left to land. The test pins the export's
//! three promises at the file it wrote and at the toast the user reads: the file is v2.1
//! holding the collection's tree, the items it skipped are listed by name, and neither a
//! secret value from the workspace files (REQ-VAR-SHARE-02, even one an untrusted file
//! carries — REQ-IO-06) nor a local-only value from memory (REQ-VAR-SHARE-01,
//! REQ-WSP-06) reaches the export.

use std::path::PathBuf;

use api_model::{Auth, Body, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody, save_item};

use crate::tree::CollectionsTree;

/// The value the fixture's untrusted collection file carries for the secret `api_key` —
/// obviously fake (`.agents/rules/tests.md`), and never allowed into an export.
const SECRET_ON_DISK: &str = "token-REDACTED-example";

/// The local-only value for `session_id`: in memory (where the local store loads it) and
/// in no workspace file — an export must never carry it out of the user's machine.
const LOCAL_VALUE: &str = "local-token-REDACTED-example";

/// The workspace under test: `pustman.json`, the collection "Users API" ordering the
/// request "Get user", the folder "Sessions" (holding "Login"), the WebSocket request
/// "Socket Echo" (a kind v2.1 cannot hold) and one id whose file is gone.
struct Fixture {
    /// Held so the workspace outlives the test.
    _temp: TempDir,
    root: PathBuf,
    collection_id: ItemId,
    /// In the collection's `order` but backed by no file — a merge artifact's shape.
    missing_id: ItemId,
}

/// Seed the workspace through `workspace_store::save_item`, except the collection file:
/// `save_item` strips a secret's value (REQ-VAR-SHARE-02), and the export must hold that
/// line even for a file that carries one — workspace files are untrusted input
/// (REQ-IO-06), so the fixture writes that one file by hand.
fn seed_workspace() -> Fixture {
    let temp = TempDir::new().expect("a scratch workspace folder");
    let root = temp.path().to_path_buf();
    let collection_id = ItemId::new();
    let request_id = ItemId::new();
    let folder_id = ItemId::new();
    let login_id = ItemId::new();
    let socket_id = ItemId::new();
    let missing_id = ItemId::new();

    save_item(
        &root,
        &Item {
            body: ItemBody::Workspace(api_model::Workspace {
                id: ItemId::new(),
                name: String::from("Export fixture"),
                format_version: String::from(workspace_store::FORMAT_VERSION),
                extra: Default::default(),
            }),
            dir: PathBuf::new(),
            path: None,
        },
    )
    .expect("the workspace file writes");

    let collection_dir = root.join("collections/users-api");
    std::fs::create_dir_all(&collection_dir).expect("the collection directory writes");
    let collection_file = serde_json::json!({
        "id": collection_id.to_string(),
        "name": "Users API",
        "order": [
            request_id.to_string(),
            folder_id.to_string(),
            socket_id.to_string(),
            missing_id.to_string(),
        ],
        "variables": [
            {"key": "api_key", "kind": "secret", "enabled": true, "value": SECRET_ON_DISK},
            {"key": "session_id", "kind": "default", "enabled": true},
        ],
    });
    std::fs::write(
        collection_dir.join("collection.json"),
        serde_json::to_string_pretty(&collection_file).expect("the fixture serializes"),
    )
    .expect("the collection file writes");

    save_item(
        &root,
        &Item {
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
                extra: Default::default(),
            })),
            dir: PathBuf::from("collections/users-api"),
            path: None,
        },
    )
    .expect("the request writes");

    save_item(
        &root,
        &Item {
            body: ItemBody::Folder(Folder {
                id: folder_id,
                name: String::from("Sessions"),
                description: None,
                auth: Auth::Inherit,
                children: Vec::new(),
                order: vec![login_id],
                extra: Default::default(),
            }),
            dir: PathBuf::from("collections/users-api"),
            path: None,
        },
    )
    .expect("the folder writes");

    save_item(
        &root,
        &Item {
            body: ItemBody::Request(Request::Http(HttpRequest {
                id: login_id,
                name: String::from("Login"),
                method: Method::Post,
                url: String::from("https://api.test/login"),
                params: Vec::new(),
                path_variables: Vec::new(),
                headers: Vec::new(),
                body: Body::None,
                auth: Auth::Inherit,
                settings: None,
                examples: Vec::new(),
                extra: Default::default(),
            })),
            dir: PathBuf::from("collections/users-api/sessions"),
            path: None,
        },
    )
    .expect("the nested request writes");

    save_item(
        &root,
        &Item {
            body: ItemBody::Request(Request::Unsupported(serde_json::json!({
                "id": socket_id.to_string(),
                "name": "Socket Echo",
                "type": "websocket",
                "request": {"url": "wss://echo.test"},
            }))),
            dir: PathBuf::from("collections/users-api"),
            path: None,
        },
    )
    .expect("the websocket request writes");

    Fixture {
        _temp: temp,
        root,
        collection_id,
        missing_id,
    }
}

/// The tree the way the shell shows it: a workspace window with the panel in its left
/// dock (the handler needs the window its toasts land on).
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
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    // A real window is active; a test window is not until asked.
    cx.update(|window, _cx| window.activate_window());
    let tree = cx.update(|window, cx| {
        let snapshot = workspace_store::open(&fixture.root).expect("the seeded workspace opens");
        let tree = cx.new(|cx| CollectionsTree::new(snapshot, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.register_panel(tree.clone(), window, cx);
        });
        tree
    });
    (workspace, tree, cx)
}

/// Every item name in a Postman `item` array, depth first — what the export holds.
fn item_names(items: &serde_json::Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(entries) = items.as_array() {
        for entry in entries {
            if let Some(name) = entry["name"].as_str() {
                names.push(name.to_owned());
            }
            names.extend(item_names(&entry["item"]));
        }
    }
    names
}

/// Whether `message` is the one toast on the workspace (the `reorder_move_tests.rs`
/// shape: one toast, under the message's own test selector).
fn toast_shows(workspace: &Entity<Workspace>, cx: &mut VisualTestContext, message: &str) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(message).leak();
    let one_toast = cx
        .read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
        == 1;
    one_toast && cx.debug_bounds(message_selector).is_some()
}

#[gpui::test]
fn export_writes_v21_and_lists_skipped(cx: &mut TestAppContext) {
    let fixture = seed_workspace();
    let (workspace, tree, cx) = open_tree(&fixture, cx);

    // The local-only value (REQ-VAR-SHARE-01) reaches app state through memory — the
    // local store loads it onto the variable and never into a workspace file
    // (REQ-WSP-06). Loading it into the tree's snapshot is how it would enter an export
    // built from that snapshot, so it is loaded here and must not survive the export.
    cx.update(|_window, cx| {
        tree.update(cx, |tree, _cx| {
            if let Some(ItemBody::Collection(collection)) = tree
                .snapshot
                .items
                .get_mut(&fixture.collection_id)
                .map(|item| &mut item.body)
            {
                for variable in &mut collection.variables {
                    if variable.key == "session_id" {
                        variable.value = Some(String::from(LOCAL_VALUE));
                    }
                }
            }
        });
    });

    let directory = tempfile::tempdir().expect("the export destination folder");
    let target = directory.path().join("users-api.postman_collection.json");

    // The row menu's Export is this handler; the dialog's destination is the only thing
    // the test chooses, as the user would.
    cx.update(|window, cx| {
        tree.update(cx, |tree, cx| {
            tree.begin_export(fixture.collection_id, window, cx);
        });
    });
    cx.simulate_new_path_selection(|_| Some(target.clone()));
    cx.run_until_parked();

    let exported = std::fs::read_to_string(&target).expect("the export writes the file");
    let document: serde_json::Value = serde_json::from_str(&exported).expect("the export is JSON");
    assert_eq!(
        document["info"]["schema"],
        "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
        "the export names the v2.1 schema (REQ-IO-03)"
    );
    assert_eq!(
        document["info"]["name"], "Users API",
        "the export is the collection the row menu opened on"
    );
    let names = item_names(&document["item"]);
    assert!(
        names.contains(&String::from("Get user")),
        "the collection's request exports: {names:?}"
    );
    assert!(
        names.contains(&String::from("Login")),
        "a nested request exports under its folder: {names:?}"
    );
    assert!(
        !names.contains(&String::from("Socket Echo")),
        "the skipped request is not in the export: {names:?}"
    );
    assert!(
        !exported.contains(SECRET_ON_DISK),
        "a secret value must not reach the export, even one a workspace file carries \
         (REQ-VAR-SHARE-02)"
    );
    assert!(
        !exported.contains(LOCAL_VALUE),
        "a local-only value must not reach the export (REQ-VAR-SHARE-01, REQ-WSP-06)"
    );

    let message = format!(
        "Exported “Users API” to {} — skipped 2 items: “Socket Echo”: a request kind v2.1 \
         cannot represent; “{}”: no folder or request file in the workspace.",
        target.display(),
        fixture.missing_id
    );
    assert!(
        toast_shows(&workspace, cx, &message),
        "the export lists the items it skipped (REQ-IO-03)"
    );
}
