//! The hostile-input half of the import (S45a fix 1): an imported Postman collection is
//! untrusted data, and every one of these tests attacks the importer the way a hostile
//! file would — keys that try to choose ids or modelled fields, names that try to escape
//! the workspace or break the filesystem, writes that fail half way, names that collide,
//! documents that nest too deep or hold too many items, and credentials that arrive as
//! literals. What must hold every time: the workspace only ever gains safe files under
//! its own root, nothing existing is overwritten, a failed import leaves nothing behind,
//! and the report tells the truth — names and keys, never values.
//!
//! These tests are wired outside [`crate::import`] so the pinned `import::` filter still
//! reports exactly the count it is pinned to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use api_model::{Auth, Body, Collection, HttpRequest, ItemId, Method, Request};
use gpui::{Entity, TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot, save_item};

use crate::import::{ImportReportModal, begin_import, too_large_message};

/// The schema URL that makes the fixtures read as Postman v2.1.
const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// Planted ids a hostile file tries to choose — real ULIDs, so any code that ever
/// adopted a file's id adopts these (fix 2's low: UUIDs were never adoptable).
const PLANTED_COLLECTION_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const PLANTED_REQUEST_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FBW";
const PLANTED_FOLDER_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FCX";
const PLANTED_RAW_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FDY";

/// The workspace folder and the import source one test runs against. The source sits
/// beside the workspace, never inside it.
struct Fixture {
    /// Held so the folders outlive the test.
    _temp: TempDir,
    root: PathBuf,
    source: PathBuf,
}

/// A workspace folder at `root` (PRD §6.1) with nothing in it yet.
fn seed_workspace(root: &Path) {
    std::fs::create_dir_all(root).expect("the workspace folder is created");
    let bytes = serde_json::to_vec_pretty(&json!({
        "id": ItemId::new().to_string(),
        "name": "Hostile fixture workspace",
        "format_version": "1.0",
    }))
    .expect("the workspace document serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file writes");
}

/// The fixture whose source holds `collection`: a scratch folder, an empty workspace and
/// one Postman file beside it.
fn seed_with(name: &str, collection: Value) -> Fixture {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    seed_workspace(&root);
    let source = temp.path().join(name);
    let bytes = serde_json::to_vec_pretty(&collection).expect("the fixture serializes");
    std::fs::write(&source, bytes).expect("the fixture writes");
    Fixture {
        _temp: temp,
        root,
        source,
    }
}

/// One window on the app's real init order — `workspace::init` gives it its
/// [`WorkspaceOpen`], `crate::init` registers the panel and the import wiring.
fn app_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    (workspace, cx)
}

/// Open `root` as this window's workspace through the one door every open lands in.
fn open_workspace_folder(workspace: &Entity<Workspace>, root: &Path, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| {
            opener.open_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();
}

/// The import of `fixture`'s source, driven through the door the file dialog's answer
/// uses — the dialog itself is the platform's and cannot appear in a test.
fn run_import(fixture: &Fixture, cx: &mut VisualTestContext) {
    let root = fixture.root.clone();
    let source = fixture.source.clone();
    cx.update(|window, cx| begin_import(root, source, window.window_handle(), cx));
    cx.run_until_parked();
    draw_window(cx);
}

/// The workspace as the disk holds it now.
fn disk(fixture: &Fixture) -> WorkspaceSnapshot {
    workspace_store::open(&fixture.root).expect("the workspace still opens")
}

/// The item called `name` in `snapshot`, wherever its file landed.
fn item_named<'a>(snapshot: &'a WorkspaceSnapshot, name: &str) -> Option<&'a Item> {
    snapshot.items.values().find(|item| item.name() == name)
}

/// One saved item's file, parsed.
fn saved_json(snapshot: &WorkspaceSnapshot, name: &str) -> Value {
    let item = item_named(snapshot, name).expect("the item landed");
    let path = item.path.as_deref().expect("the item has a file");
    let bytes = std::fs::read(snapshot.root.join(path)).expect("the file reads");
    serde_json::from_slice(&bytes).expect("the file is JSON")
}

/// The ids in a container's saved `order`.
fn order_of(value: &Value) -> Vec<String> {
    value["order"]
        .as_array()
        .map(|order| {
            order
                .iter()
                .map(|id| id.as_str().expect("an id is a string").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `message` is the one toast on screen (the `toast_layer_tests` shape: the
/// message key must outlive the frame `debug_bounds` reads, so the selector is leaked).
fn toast_shows(workspace: &Entity<Workspace>, cx: &mut VisualTestContext, message: &str) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(message).leak();
    let one_toast = cx
        .read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
        == 1;
    one_toast && cx.debug_bounds(message_selector).is_some()
}

/// Whether the report modal is open.
fn report_is_open(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<ImportReportModal>()
                .is_some()
        })
    })
}

/// The report's warnings, read off the open modal.
fn report_warnings(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    let modal = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace
                .modal_layer()
                .active_modal::<ImportReportModal>()
                .expect("the report modal is open")
        })
    });
    cx.read(|cx| {
        modal.read_with(cx, |modal, _| {
            modal
                .warnings()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
    })
}

/// The names in the directory `path` holds, sorted.
fn entries_of(path: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(path)
        .expect("the directory reads")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

// ---------------------------------------------------------------------------------------
// 1. Untrusted keys never choose identity or modelled fields.

/// A collection whose every level carries keys that would, unchecked, overwrite the
/// mapped fields when the file is saved: `settings` would turn TLS off, `url` would
/// retarget the request, `auth` would plant credentials, `id` would collide with another
/// item or be unreadable (a Postman UUID is not a ULID), `order`/`variables` would
/// rewrite the tree.
fn hostile_keys_json() -> Value {
    json!({
        "info": { "name": "Hostile API", "schema": SCHEMA },
        "id": PLANTED_COLLECTION_ID,
        "order": ["stolen"],
        "variables": [],
        "settings": { "verify_tls": false },
        "item": [
            {
                "name": "Hostile request",
                "id": PLANTED_REQUEST_ID,
                "settings": { "verify_tls": false },
                "url": "https://evil.test/",
                "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "stolen" }] },
                "order": ["stolen"],
                "request": { "method": "GET", "url": "https://api.test/real" }
            },
            {
                "name": "Hostile folder",
                "id": PLANTED_FOLDER_ID,
                "order": ["stolen"],
                "settings": { "verify_tls": false },
                "item": [
                    { "name": "Inner", "request": { "method": "GET", "url": "https://api.test/inner" } }
                ]
            },
            {
                "name": "Raw one",
                "id": PLANTED_RAW_ID,
                "request": "https://api.test/raw"
            },
            {
                "name": "Raw two",
                "id": PLANTED_RAW_ID,
                "request": "https://api.test/raw"
            }
        ]
    })
}

#[gpui::test]
fn hostile_keys_cannot_choose_ids_or_modelled_fields(cx: &mut TestAppContext) {
    let fixture = seed_with("hostile.postman_collection.json", hostile_keys_json());
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    let collection = saved_json(&snapshot, "Hostile API");
    let request = saved_json(&snapshot, "Hostile request");
    let folder = saved_json(&snapshot, "Hostile folder");
    let inner = saved_json(&snapshot, "Inner");
    let raw_one = saved_json(&snapshot, "Raw one");
    let raw_two = saved_json(&snapshot, "Raw two");

    // The mapped values win, and no `settings`/`url`/`auth`/`order`/`variables` key from
    // the file survives into the saved JSON: no extra key naming a modelled field does.
    assert_eq!(
        request["url"], "https://api.test/real",
        "the saved request carries the mapped URL, not the file's"
    );
    for (what, value) in [
        ("request", &request),
        ("folder", &folder),
        ("collection", &collection),
    ] {
        assert!(
            value.get("settings").is_none(),
            "the {what} carries no settings the file chose"
        );
    }
    assert!(
        request.get("auth").is_none() && request.get("order").is_none(),
        "the request carries no auth or order the file chose"
    );
    assert!(
        collection.get("variables").is_none(),
        "the collection carries no variables the file chose"
    );
    assert_eq!(
        raw_one["postman_raw"]["request"], "https://api.test/raw",
        "a preserved request keeps its payload verbatim, under the wrapper"
    );
    assert_eq!(raw_one["name"], "Raw one", "and its name");

    // Every id is a fresh ULID of our minting: readable, unique, and none of the file's.
    let ids = [
        &collection["id"],
        &request["id"],
        &folder["id"],
        &inner["id"],
        &raw_one["id"],
        &raw_two["id"],
    ];
    for id in &ids {
        assert!(
            id.as_str()
                .and_then(|id| id.parse::<ItemId>().ok())
                .is_some(),
            "every saved id is a ULID this build can read: {id}"
        );
    }
    let unique = ids
        .iter()
        .map(|id| id.as_str().expect("an id is a string").to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), ids.len(), "and no two items share one");
    let planted = [
        PLANTED_COLLECTION_ID,
        PLANTED_REQUEST_ID,
        PLANTED_FOLDER_ID,
        PLANTED_RAW_ID,
    ];
    assert!(
        ids.iter()
            .all(|id| !planted.contains(&id.as_str().expect("an id"))),
        "and none of them is the ULID the file tried to plant"
    );

    // The tree the files describe is the one the import mapped.
    let landed = [
        &request["id"],
        &folder["id"],
        &raw_one["id"],
        &raw_two["id"],
    ]
    .map(|id| id.as_str().expect("an id").to_owned())
    .to_vec();
    assert_eq!(
        order_of(&collection),
        landed,
        "the collection's order is its landed children"
    );
    assert_eq!(
        order_of(&folder),
        vec![inner["id"].as_str().expect("an id").to_owned()],
        "the folder's order is its landed child"
    );

    // The report says every ignored key was ignored — keys only, never their values.
    assert!(report_is_open(&workspace, cx), "the report modal is open");
    let warnings = report_warnings(&workspace, cx);
    let ignored = warnings
        .iter()
        .filter(|warning| warning.contains("ignored"))
        .collect::<Vec<_>>();
    assert_eq!(
        ignored.len(),
        14,
        "every ignored key is reported once: {warnings:?}"
    );
    for key in ["id", "settings", "url", "auth", "order", "variables"] {
        assert!(
            ignored.iter().any(|warning| warning.contains(key)),
            "the report names the ignored {key} key"
        );
    }
    let report_text = warnings.join("\n");
    assert!(
        !report_text.contains("evil.test") && !report_text.contains("stolen"),
        "the report names keys, never the values behind them"
    );
}

// ---------------------------------------------------------------------------------------
// 2. Hostile names cannot escape the workspace or break the filesystem.

#[gpui::test]
fn hostile_names_stay_inside_the_workspace(cx: &mut TestAppContext) {
    let collection = json!({
        "info": { "name": "Hostile names", "schema": SCHEMA },
        "item": [
            { "name": "../../escape", "request": { "method": "GET", "url": "https://api.test/a" } },
            { "name": "/etc/passwd", "request": { "method": "GET", "url": "https://api.test/b" } },
            { "name": "CON", "request": { "method": "GET", "url": "https://api.test/c" } },
            { "name": "NUL", "request": { "method": "GET", "url": "https://api.test/d" } },
            { "name": "a".repeat(300), "request": { "method": "GET", "url": "https://api.test/e" } }
        ]
    });
    let fixture = seed_with("names.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    for item in snapshot.items.values() {
        if matches!(item.body, ItemBody::Workspace(_)) {
            continue;
        }
        let Some(path) = item.path.as_deref() else {
            continue;
        };
        let shown = path.to_string_lossy();
        assert!(
            shown.starts_with("collections/hostile-names/"),
            "{shown} lands under the collection's own directory"
        );
        assert!(
            !shown.contains("..")
                && path
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "{shown} is built of plain names only"
        );
    }

    // Nothing outside the workspace folder was touched: the scratch folder still holds
    // exactly the workspace and the import source.
    let outside = fixture.root.parent().expect("the workspace has a parent");
    assert_eq!(
        entries_of(outside),
        vec![
            "names.postman_collection.json".to_owned(),
            "workspace".to_owned()
        ],
        "the import wrote nothing beside the workspace"
    );

    // The slugs are safe: traversal names fold to plain names, device names are adjusted,
    // and the overlong name lands as a file name the filesystem accepts.
    let paths = snapshot
        .items
        .values()
        .filter_map(|item| item.path.clone())
        .collect::<Vec<_>>();
    for expected in [
        "collections/hostile-names/escape.request.json",
        "collections/hostile-names/etc-passwd.request.json",
        "collections/hostile-names/con-imported.request.json",
        "collections/hostile-names/nul-imported.request.json",
    ] {
        assert!(
            paths.iter().any(|path| path == Path::new(expected)),
            "{expected} is where the safe slug puts it"
        );
    }
    let overlong = paths
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("aaaa"))
        })
        .expect("the overlong name landed as a file");
    assert!(
        overlong.file_name().expect("a file name").len() < 255,
        "an overlong name becomes a file name the filesystem accepts"
    );
    assert!(
        report_is_open(&workspace, cx),
        "and the report is open, naming what was adjusted"
    );
    assert_eq!(
        report_warnings(&workspace, cx).len(),
        3,
        "the two device names and the overlong name are reported"
    );
}

// ---------------------------------------------------------------------------------------
// 2. A write that fails half way rolls everything back.

#[gpui::test]
fn failed_import_leaves_nothing_and_reports_the_reason(cx: &mut TestAppContext) {
    let collection = json!({
        "info": { "name": "Pets API", "schema": SCHEMA },
        "item": [
            {
                "name": "Admin",
                "item": [
                    { "name": "Inside", "request": { "method": "GET", "url": "https://api.test/in" } }
                ]
            },
            { "name": "trap", "request": { "method": "GET", "url": "https://api.test/trap" } }
        ]
    });
    let fixture = seed_with("pets.postman_collection.json", collection);
    // The trap: every file name the "trap" request could take is already a file of its
    // own, so its write cannot land and the import fails after several writes did.
    let pets_dir = fixture.root.join("collections/pets-api");
    std::fs::create_dir_all(&pets_dir).expect("the trap directory is created");
    for attempt in 1..=999 {
        let stem = if attempt == 1 {
            String::from("trap")
        } else {
            format!("trap-{attempt}")
        };
        std::fs::write(pets_dir.join(format!("{stem}.request.json")), b"")
            .expect("the trap file is written");
    }
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    assert_eq!(
        snapshot.items.len(),
        1,
        "a failed import leaves no item behind — only the workspace item is there"
    );
    assert_eq!(
        entries_of(&pets_dir).len(),
        999,
        "the collection's directory holds exactly the trap files again"
    );
    assert!(
        entries_of(&pets_dir)
            .iter()
            .all(|name| name.starts_with("trap")),
        "and nothing of the import's own files"
    );
    assert!(
        !pets_dir.join("admin").exists(),
        "the slug directory the import created is rolled back too"
    );
    assert!(
        cx.read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<ImportReportModal>()
                    .is_none()
            })
        }),
        "a failed import earns no report"
    );
    assert!(
        toast_shows(
            &workspace,
            cx,
            "Could not write “trap” into the workspace: no free file name for trap"
        ),
        "and one truthful message names the item and the reason"
    );
}

// ---------------------------------------------------------------------------------------
// 2. A name that is already taken is suffixed, never overwritten.

/// A workspace that already holds a collection named "Pets API" with one request in it.
fn seed_existing_collection(fixture: &Fixture) -> Vec<u8> {
    let collection_id = ItemId::new();
    let marker_id = ItemId::new();
    let collection = Item {
        body: ItemBody::Collection(Collection {
            id: collection_id,
            name: String::from("Pets API"),
            description: None,
            auth: Auth::Inherit,
            variables: Vec::new(),
            order: vec![marker_id],
            children: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("collections"),
        path: None,
    };
    save_item(&fixture.root, &collection).expect("the existing collection writes");
    let marker = Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: marker_id,
            name: String::from("Marker"),
            method: Method::Get,
            url: String::from("https://api.test/marker"),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections/pets-api"),
        path: None,
    };
    save_item(&fixture.root, &marker).expect("the existing request writes");
    std::fs::read(fixture.root.join("collections/pets-api/collection.json"))
        .expect("the existing collection reads back")
}

#[gpui::test]
fn same_name_import_lands_beside_the_existing_collection(cx: &mut TestAppContext) {
    let collection = json!({
        "info": { "name": "Pets API", "schema": SCHEMA },
        "item": [
            { "name": "Get pets", "request": { "method": "GET", "url": "https://api.test/pets" } }
        ]
    });
    let fixture = seed_with("pets.postman_collection.json", collection);
    let existing_bytes = seed_existing_collection(&fixture);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    assert_eq!(
        std::fs::read(fixture.root.join("collections/pets-api/collection.json"))
            .expect("the existing collection still reads"),
        existing_bytes,
        "nothing that exists is overwritten"
    );
    let snapshot = disk(&fixture);
    let pets = snapshot
        .items
        .values()
        .filter(|item| item.name() == "Pets API")
        .collect::<Vec<_>>();
    assert_eq!(
        pets.len(),
        2,
        "the workspace now holds two collections named Pets API"
    );
    let ids = pets
        .iter()
        .map(|item| item.id().expect("a collection carries its id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 2, "with two distinct ids");
    let imported = pets
        .iter()
        .find(|item| {
            item.path
                .as_deref()
                .is_some_and(|path| path.starts_with("collections/pets-api-2/"))
        })
        .expect("the imported collection lands suffixed");
    assert_eq!(
        snapshot
            .items
            .values()
            .filter(|item| item.name() == "Get pets")
            .filter_map(|item| item.path.clone())
            .collect::<Vec<_>>(),
        vec![PathBuf::from(
            "collections/pets-api-2/get-pets.request.json"
        )],
        "and its request lands beside it, not inside the existing collection"
    );
    let ItemBody::Collection(imported) = &imported.body else {
        panic!("the landed item is a collection");
    };
    let child = snapshot
        .items
        .get(&imported.order[0])
        .expect("the imported collection's order names its child");
    assert_eq!(
        child.name(),
        "Get pets",
        "the suffixed collection orders its own request"
    );
}

// ---------------------------------------------------------------------------------------
// 2. Nesting over the cap is refused with a message.

#[gpui::test]
fn nesting_over_the_cap_is_refused_with_a_message(cx: &mut TestAppContext) {
    let mut item =
        json!({ "name": "Deep", "request": { "method": "GET", "url": "https://api.test/" } });
    for _ in 0..65 {
        item = json!({ "name": "Level", "item": [item] });
    }
    let collection = json!({ "info": { "name": "Deep API", "schema": SCHEMA }, "item": [item] });
    let fixture = seed_with("deep.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    assert_eq!(
        snapshot.items.len(),
        1,
        "nothing is written from a document nesting past the cap"
    );
    assert!(
        toast_shows(
            &workspace,
            cx,
            "Could not import deep.postman_collection.json: the JSON nests deeper than 64 levels."
        ),
        "and the refusal names the file and the limit"
    );
}

// ---------------------------------------------------------------------------------------
// 5. The item cap.

#[gpui::test]
fn item_cap_is_enforced_with_a_message(cx: &mut TestAppContext) {
    let items = (0..=10_000)
        .map(|index| {
            json!({ "name": format!("R{index}"), "request": { "method": "GET", "url": "https://api.test/" } })
        })
        .collect::<Vec<_>>();
    let collection = json!({ "info": { "name": "Huge API", "schema": SCHEMA }, "item": items });
    let fixture = seed_with("huge.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    assert_eq!(
        snapshot.items.len(),
        1,
        "an import over the item cap writes nothing"
    );
    assert!(
        toast_shows(
            &workspace,
            cx,
            "Could not import huge.postman_collection.json: the collection holds more than 10000 items."
        ),
        "and the refusal names the file and the cap"
    );
}

// ---------------------------------------------------------------------------------------
// 5. Literal credentials are reported by name, never by value.

#[gpui::test]
fn literal_credentials_are_reported_by_name_only(cx: &mut TestAppContext) {
    let collection = json!({
        "info": { "name": "Credentials API", "schema": SCHEMA },
        "auth": {
            "type": "basic",
            "basic": [
                { "key": "username", "value": "someone" },
                { "key": "password", "value": "collection-pass-REDACTED-example" }
            ]
        },
        "item": [
            {
                "name": "Login",
                "request": {
                    "method": "POST",
                    "url": "https://api.test/login",
                    "header": [
                        { "key": "Authorization", "value": "Bearer sk-literal-REDACTED-example" }
                    ]
                }
            },
            {
                "name": "Basic login",
                "request": {
                    "method": "GET",
                    "url": "https://api.test/x",
                    "auth": {
                        "type": "basic",
                        "basic": [
                            { "key": "username", "value": "someone" },
                            { "key": "password", "value": "hunter2-REDACTED-example" }
                        ]
                    }
                }
            },
            {
                "name": "Almost templated",
                "request": {
                    "method": "GET",
                    "url": "https://api.test/z",
                    "header": [
                        { "key": "Authorization", "value": "Bearer {{token}}" }
                    ]
                }
            },
            {
                "name": "Placeholder",
                "request": {
                    "method": "GET",
                    "url": "https://api.test/y",
                    "header": [
                        { "key": "Authorization", "value": "{{token}}" }
                    ]
                }
            },
            {
                "name": "Url creds",
                "request": {
                    "method": "GET",
                    "url": "https://user:pass-REDACTED-example@api.test/a"
                }
            },
            {
                "name": "Query creds",
                "request": {
                    "method": "GET",
                    "url": "https://api.test/b?api_key=key-literal-REDACTED-example"
                }
            },
            {
                "name": "Param creds",
                "request": {
                    "method": "GET",
                    "url": "https://api.test/c?password=param-literal-REDACTED-example"
                }
            },
            {
                "name": "Tokens",
                "auth": {
                    "type": "bearer",
                    "bearer": [{ "key": "token", "value": "folder-token-REDACTED-example" }]
                },
                "item": [
                    { "name": "Folder child", "request": { "method": "GET", "url": "https://api.test/d" } }
                ]
            }
        ]
    });
    let fixture = seed_with("credentials.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let warnings = report_warnings(&workspace, cx);
    assert_eq!(
        warnings.len(),
        8,
        "every literal credential is reported once: {warnings:?}"
    );
    for name in [
        "Credentials API",
        "Login",
        "Basic login",
        "Almost templated",
        "Url creds",
        "Query creds",
        "Param creds",
        "Tokens",
    ] {
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains(name) && warning.contains("plain text")),
            "{name} is named as carrying a credential imported as plain text"
        );
    }
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("Placeholder") || warning.contains("Folder child")),
        "a value that is one well-formed variable reference — and an uninvolved request — is not flagged"
    );
    let report_text = warnings.join("\n");
    for secret in [
        "collection-pass-REDACTED-example",
        "sk-literal-REDACTED-example",
        "hunter2-REDACTED-example",
        "pass-REDACTED-example",
        "key-literal-REDACTED-example",
        "param-literal-REDACTED-example",
        "folder-token-REDACTED-example",
    ] {
        assert!(
            !report_text.contains(secret),
            "the report names requests, never credential values ({secret})"
        );
    }
    // The request itself keeps what the file said: the warning is advice, not a rewrite.
    let snapshot = disk(&fixture);
    let login = item_named(&snapshot, "Login").expect("the request landed");
    let bytes = std::fs::read(snapshot.root.join(login.path.as_deref().expect("a file")))
        .expect("the file reads");
    assert!(
        bytes
            .windows(b"sk-literal-REDACTED-example".len())
            .any(|window| window == b"sk-literal-REDACTED-example"),
        "the imported request carries its header verbatim"
    );
}

// ---------------------------------------------------------------------------------------
// 1. A preserved item can never load back as a live request.

/// A preserved-verbatim item shaped like a live request: written as-is it would load
/// back as a typed request pointed at `https://evil.test` with TLS verification off —
/// sendable, with a report that says only "preserved verbatim".
fn preserved_payload_json() -> Value {
    json!({
        "info": { "name": "Preserved API", "schema": SCHEMA },
        "item": [
            {
                "name": "Evil",
                "id": PLANTED_RAW_ID,
                "type": "http",
                "method": "GET",
                "url": "https://evil.test/",
                "settings": { "verify_tls": false },
                "password": "hunter2-REDACTED-example",
                "request": "https://api.test/raw"
            }
        ]
    })
}

#[gpui::test]
fn preserved_items_never_load_as_live_requests(cx: &mut TestAppContext) {
    let fixture = seed_with(
        "preserved.postman_collection.json",
        preserved_payload_json(),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    // The reopened item is inert data again — not an HttpRequest with a target.
    let snapshot = disk(&fixture);
    let item = item_named(&snapshot, "Evil").expect("the item landed");
    assert!(
        matches!(&item.body, ItemBody::Request(Request::Unsupported(_))),
        "the preserved item does not reopen as a typed request"
    );
    let saved = saved_json(&snapshot, "Evil");
    for key in [
        "type", "method", "url", "auth", "settings", "body", "headers",
    ] {
        assert!(
            saved.get(key).is_none(),
            "no live-request field sits at the top level: {key}"
        );
    }
    assert_eq!(
        saved["postman_raw"]["url"], "https://evil.test/",
        "the payload is preserved verbatim under the wrapper"
    );
    assert!(
        saved["id"]
            .as_str()
            .and_then(|id| id.parse::<ItemId>().ok())
            .is_some(),
        "the file's id is one of ours"
    );
    assert_ne!(
        saved["id"], PLANTED_RAW_ID,
        "and not the one the file tried to plant"
    );

    // The credential check runs on preserved items too — by name, never by value.
    let warnings = report_warnings(&workspace, cx);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("Evil") && warning.contains("plain text")),
        "the preserved item's literal credential is reported"
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("hunter2-REDACTED-example")),
        "and its value never reaches the report"
    );
}

// ---------------------------------------------------------------------------------------
// 2. A failed import takes back every directory it created.

#[gpui::test]
fn failed_import_removes_every_directory_it_created(cx: &mut TestAppContext) {
    // The trap is the import's own doing: 1000 requests named "trap" exhaust every file
    // name the last one could take (`trap.request.json`, `trap-2` … `trap-999`), so the
    // import fails only after it created `collections/` and the collection's slug
    // directory — both of which the rollback owes back.
    let items = (0..1000)
        .map(|_| {
            json!({ "name": "trap", "request": { "method": "GET", "url": "https://api.test/" } })
        })
        .collect::<Vec<_>>();
    let collection = json!({ "info": { "name": "Pets API", "schema": SCHEMA }, "item": items });
    let fixture = seed_with("pets.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    assert_eq!(
        snapshot.items.len(),
        1,
        "a failed import leaves no item behind"
    );
    assert!(
        !fixture.root.join("collections").exists(),
        "and no directory it created — `collections/` included"
    );
    assert!(
        toast_shows(
            &workspace,
            cx,
            "Could not write “trap” into the workspace: no free file name for trap"
        ),
        "one truthful message names the item and the reason"
    );
}

// ---------------------------------------------------------------------------------------
// 3. A file over the cap is refused without a partial import.

#[gpui::test]
fn oversized_file_is_refused_without_a_partial_import(cx: &mut TestAppContext) {
    let collection = json!({
        "info": { "name": "Huge API", "schema": SCHEMA },
        "item": [
            { "name": "First", "request": { "method": "GET", "url": "https://api.test/" } }
        ]
    });
    let fixture = seed_with("huge.postman_collection.json", collection);
    // Grow the source past the cap without holding 50 MB anywhere: the head is still a
    // valid collection, so anything short of a refusal would import part of it.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&fixture.source)
        .expect("the source opens");
    file.set_len(50 * 1024 * 1024 + 1)
        .expect("the source grows past the cap");
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = disk(&fixture);
    assert_eq!(
        snapshot.items.len(),
        1,
        "no partial import of an oversized file"
    );
    assert!(
        !fixture.root.join("collections").exists(),
        "and not one file written"
    );
    assert!(
        cx.read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<ImportReportModal>()
                    .is_none()
            })
        }),
        "an oversized file earns no report"
    );
    assert!(
        toast_shows(&workspace, cx, &too_large_message(&fixture.source)),
        "and the refusal names the file and the limit"
    );
}

// ---------------------------------------------------------------------------------------
// 1. Preserved items carry the same credential notice as typed ones.

#[gpui::test]
fn preserved_credentials_are_named_in_the_notice(cx: &mut TestAppContext) {
    // Three preserved requests — no `method`, so the importer keeps them verbatim — each
    // carrying a credential the notice must catch with the same checks the typed path
    // makes: a credential header, a `user:pass@` URL, and an unsupported auth holding a
    // literal key (the repo's hawk shape).
    let collection = json!({
        "info": { "name": "Notice API", "schema": SCHEMA },
        "item": [
            {
                "name": "Header creds",
                "request": {
                    "url": "https://api.test/h",
                    "header": [
                        { "key": "Authorization", "value": "Bearer authz-literal-REDACTED-example" }
                    ]
                }
            },
            {
                "name": "Url creds raw",
                "request": {
                    "url": "https://user:pass-REDACTED-example@api.test/u"
                }
            },
            {
                "name": "Auth creds raw",
                "request": {
                    "auth": { "type": "hawk", "key": "token-literal-REDACTED-example" }
                }
            }
        ]
    });
    let fixture = seed_with("notice.postman_collection.json", collection);
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let warnings = report_warnings(&workspace, cx);
    assert_eq!(
        warnings.len(),
        6,
        "three credential notices beside the three 'preserved verbatim' notes: {warnings:?}"
    );
    for name in ["Header creds", "Url creds raw", "Auth creds raw"] {
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains(name) && warning.contains("plain text")),
            "{name} is named as carrying a credential imported as plain text"
        );
    }
    let report_text = warnings.join("\n");
    for secret in [
        "authz-literal-REDACTED-example",
        "pass-REDACTED-example",
        "token-literal-REDACTED-example",
    ] {
        assert!(
            !report_text.contains(secret),
            "the report names requests, never credential values ({secret})"
        );
    }
}

// ---------------------------------------------------------------------------------------
// 2/3. A directory that cannot be listed is kept and reported — once.

/// The unlistable-directory path: a directory without read permission is never claimed
/// by the rollback and the reason is reported. Permissions do not bind root, so under
/// root the test steps aside with a message instead of pretending to cover the path.
#[cfg(unix)]
#[gpui::test]
fn unlistable_directory_is_kept_and_reported(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;

    let collection = json!({
        "info": { "name": "Pets API", "schema": SCHEMA },
        "item": [
            { "name": "Get pets", "request": { "method": "GET", "url": "https://api.test/pets" } }
        ]
    });
    let fixture = seed_with("pets.postman_collection.json", collection);
    let collections = fixture.root.join("collections");
    std::fs::create_dir_all(&collections).expect("the collections directory is created");
    // Write + search but no read: the import can still create `pets-api/` inside, while
    // listing the directory (what the rollback needs) fails. `0o111` would also block the
    // write, and the test could then only fail.
    let unlistable = std::fs::Permissions::from_mode(0o311);
    std::fs::set_permissions(&collections, unlistable)
        .expect("the directory loses read permission");
    if std::fs::read_dir(&collections).is_ok() {
        // Restore before stepping aside so the scratch folder can clean up.
        let _restored =
            std::fs::set_permissions(&collections, std::fs::Permissions::from_mode(0o755));
        eprintln!(
            "unlistable_directory_is_kept_and_reported skipped: \
             directory permissions do not bind this user (root)"
        );
        return;
    }
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    // The import itself still lands: an unreadable listing says nothing about writing.
    assert!(
        collections.join("pets-api/collection.json").exists(),
        "the collection is written all the same"
    );
    let warnings = report_warnings(&workspace, cx);
    assert_eq!(
        warnings.len(),
        1,
        "the unlistable directory is reported exactly once, not once per save: {warnings:?}"
    );
    assert!(
        warnings[0].starts_with("collections") && warnings[0].contains("kept"),
        "the report names the directory and says it is kept"
    );
    assert!(collections.exists(), "and the directory is kept as it is");
    // Give the permission back so the scratch folder can clean up.
    let _restored = std::fs::set_permissions(&collections, std::fs::Permissions::from_mode(0o755));
}
