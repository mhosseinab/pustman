//! Importing a Postman collection end to end (S45a): the untrusted file the user picked
//! becomes workspace files and a report — and never anything else. The two tests walk
//! the step's two promises: an import writes every item through `workspace_store` and
//! opens the report (with the scripts it preserved inert and the secret it never wrote),
//! and a file over the size cap is rejected with a message before it is read.

use std::path::{Path, PathBuf};

use api_model::{ItemId, VariableKind};
use gpui::{Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use super::{ImportReportModal, begin_import, too_large_message};

/// The fake secret the fixture carries — obviously fake, so secret scanners and
/// reviewers never trip on it (`.agents/rules/tests.md`).
const FIXTURE_SECRET: &str = "token-REDACTED-example";

/// The workspace folder and the import source the tests run against. Both live in one
/// scratch folder; the source sits beside the workspace, never inside it.
struct Fixture {
    /// Held so the folders outlive the test.
    _temp: TempDir,
    root: PathBuf,
    source: PathBuf,
}

/// The Postman v2.1 collection the fixture file holds: one request with a `prerequest`
/// script, one folder with a request inside it, an auth type this build does not model,
/// and a `secret`-typed variable whose value must never reach a workspace file.
fn pets_collection_json() -> serde_json::Value {
    serde_json::json!({
        "info": {
            "name": "Pets API",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "auth": { "type": "oauth1" },
        "item": [
            {
                "name": "Get pets",
                "event": [
                    {
                        "listen": "prerequest",
                        "script": { "type": "text/javascript", "exec": ["console.log('inert')"] }
                    }
                ],
                "request": { "method": "GET", "url": "https://api.test/pets" }
            },
            {
                "name": "Admin",
                "item": [
                    {
                        "name": "Delete pet",
                        "request": { "method": "DELETE", "url": "https://api.test/pets/1" }
                    }
                ]
            }
        ],
        "variable": [{ "key": "api_token", "type": "secret", "value": FIXTURE_SECRET }]
    })
}

/// A workspace folder at `root` (PRD §6.1) with nothing in it yet.
fn seed_workspace(root: &Path) {
    std::fs::create_dir_all(root).expect("the workspace folder is created");
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Import fixture workspace",
        "format_version": "1.0",
    }))
    .expect("the workspace document serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file writes");
}

/// The fixture whose source holds [`pets_collection_json`].
fn seed() -> Fixture {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    seed_workspace(&root);
    let source = temp.path().join("pets.postman_collection.json");
    let bytes = serde_json::to_vec_pretty(&pets_collection_json()).expect("the fixture serializes");
    std::fs::write(&source, bytes).expect("the fixture writes");
    Fixture {
        _temp: temp,
        root,
        source,
    }
}

/// The fixture whose source is over the 50 MB import cap: a sparse file, so the test
/// never holds 50 MB anywhere.
fn seed_oversized() -> Fixture {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    seed_workspace(&root);
    let source = temp.path().join("huge.postman_collection.json");
    let file = std::fs::File::create(&source).expect("the oversized fixture is created");
    let over_the_cap = 50 * 1024 * 1024 + 1;
    file.set_len(over_the_cap)
        .expect("the fixture grows past the cap");
    Fixture {
        _temp: temp,
        root,
        source,
    }
}

/// One window on the app's real init order — `workspace::init` gives it its
/// [`WorkspaceOpen`], `crate::init` registers the panel and this step's import wiring.
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
}

/// The item called `name` in `snapshot`, wherever its file landed.
fn item_named<'a>(snapshot: &'a WorkspaceSnapshot, name: &str) -> Option<&'a Item> {
    snapshot.items.values().find(|item| item.name() == name)
}

/// The names a container's `order` names, in order (REQ-WSP-01).
fn order_names(snapshot: &WorkspaceSnapshot, container: &Item) -> Vec<String> {
    let order = match &container.body {
        ItemBody::Collection(collection) => &collection.order,
        ItemBody::Folder(folder) => &folder.order,
        _ => return Vec::new(),
    };
    order
        .iter()
        .filter_map(|id| snapshot.items.get(id))
        .map(Item::name)
        .collect()
}

/// Whether any file under `root` holds `needle` — the high-stakes promise: an imported
/// secret value never lands in a workspace file.
fn any_file_under_contains(root: &Path, needle: &str) -> bool {
    fn walk(dir: &Path, needle: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path, needle) {
                    return true;
                }
            } else if let Ok(bytes) = std::fs::read(&path)
                && String::from_utf8_lossy(&bytes).contains(needle)
            {
                return true;
            }
        }
        false
    }
    walk(root, needle)
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

/// The report's four sections as the tests read them.
struct Sections {
    imported: Vec<String>,
    scripts: Vec<String>,
    auth: Vec<String>,
    warnings: Vec<String>,
}

/// The report modal's four sections, read off the open modal.
fn report_sections(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<Sections> {
    let modal = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<ImportReportModal>()
        })
    })?;
    Some(cx.read(|cx| {
        modal.read_with(cx, |modal, _| {
            let strings = |lines: &[gpui::SharedString]| {
                lines.iter().map(ToString::to_string).collect::<Vec<_>>()
            };
            Sections {
                imported: strings(modal.imported()),
                scripts: strings(modal.preserved_scripts()),
                auth: strings(modal.unsupported_auth()),
                warnings: strings(modal.warnings()),
            }
        })
    }))
}

#[gpui::test]
fn import_collection_writes_files_and_shows_report(cx: &mut TestAppContext) {
    let fixture = seed();
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = workspace_store::open(&fixture.root).expect("the workspace still opens");
    let collection = item_named(&snapshot, "Pets API").expect("the collection landed");
    assert_eq!(
        snapshot.path(&collection.id().expect("the collection carries its id")),
        Some(Path::new("collections/pets-api/collection.json")),
        "the collection is written under its own slug"
    );
    assert_eq!(
        order_names(&snapshot, collection),
        vec!["Get pets", "Admin"],
        "the collection's order names its imported children"
    );
    let admin = item_named(&snapshot, "Admin").expect("the folder landed");
    assert_eq!(
        snapshot.path(&admin.id().expect("the folder carries its id")),
        Some(Path::new("collections/pets-api/admin/folder.json")),
        "the folder lands inside the collection's slug directory"
    );
    assert_eq!(
        order_names(&snapshot, admin),
        vec!["Delete pet"],
        "the folder's order names its request"
    );
    assert_eq!(
        snapshot.path(
            &item_named(&snapshot, "Delete pet")
                .and_then(Item::id)
                .expect("the request carries its id")
        ),
        Some(Path::new(
            "collections/pets-api/admin/delete-pet.request.json"
        )),
        "the nested request lands inside the folder's slug directory"
    );

    // The high-stakes promise: the imported `secret` variable keeps no value anywhere in
    // the workspace (REQ-VAR-SHARE-02), and none of it reaches the report either.
    let ItemBody::Collection(imported) = &collection.body else {
        panic!("the landed item is the imported collection");
    };
    let api_token = imported
        .variables
        .iter()
        .find(|variable| variable.key == "api_token")
        .expect("the collection's variable was imported");
    assert_eq!(api_token.kind, VariableKind::Secret, "as a secret");
    assert_eq!(api_token.value, None, "keeping no value in the model");
    assert!(
        !any_file_under_contains(&fixture.root, FIXTURE_SECRET),
        "and no workspace file carries the imported secret value"
    );

    let report = report_sections(&workspace, cx).expect("the report modal is open");
    assert_eq!(
        report.imported,
        vec!["Pets API", "Get pets", "Admin", "Delete pet"],
        "the report lists every imported item"
    );
    assert_eq!(
        report.scripts.len(),
        1,
        "the report names the preserved script"
    );
    assert!(
        report.scripts[0].contains("not executed"),
        "as preserved but never executed, not as something that ran"
    );
    assert_eq!(
        report.auth.len(),
        1,
        "the report names the unsupported auth"
    );
    assert!(
        report.auth[0].contains("oauth1"),
        "naming the auth type this build did not map"
    );
    assert!(
        report.warnings.is_empty(),
        "and nothing else needed a warning"
    );
    let report_text = report
        .imported
        .iter()
        .chain(report.scripts.iter())
        .chain(report.auth.iter())
        .chain(report.warnings.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !report_text.contains(FIXTURE_SECRET),
        "the report never echoes an imported secret value"
    );
}

#[gpui::test]
fn oversized_file_rejected_with_message(cx: &mut TestAppContext) {
    let fixture = seed_oversized();
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);

    run_import(&fixture, cx);

    let snapshot = workspace_store::open(&fixture.root).expect("the workspace still opens");
    assert_eq!(
        snapshot.items.len(),
        1,
        "nothing is written from a file over the cap — only the workspace item is there"
    );
    assert!(
        !fixture.root.join("collections").exists(),
        "and no collection directory appears"
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
        "a rejected file earns no report"
    );
    assert!(
        toast_shows(&workspace, cx, &too_large_message(&fixture.source)),
        "and the rejection reaches the user as a message naming the limit"
    );
}
