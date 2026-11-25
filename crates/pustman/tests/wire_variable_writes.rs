//! The `{{var}}` popover's writes in the running app (S48u6 fix 1): the scope wire gives
//! every bound request editor a writer that lands an edit in the scope it names — the
//! active environment's file, `pustman.json`'s globals or the request's collection — and
//! an editor not bound to the open workspace gets none, so a tab left open from another
//! folder can never write into the folder now open, nor the new folder into the old one.
//!
//! Booted through the real `pustman::init_with_store` over an in-memory store with a
//! temp workspace folder opened through `WorkspaceOpen`. Every value is obviously fake.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{
    Auth, Body, Collection, Environment, HttpRequest, ItemId, Method, Request, Variable,
    VariableKind,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use pustman_actions::quick_open::Toggle as ToggleQuickOpen;
use request_editor::{EditChange, RequestEditor, VariableEdit, VariableWrites};
use storage::LocalStore;
use ui::test_support::draw_window;
use variables::ScopeKind;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

const REQUEST_NAME: &str = "List pets";

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

fn save(root: &Path, item: Item) -> PathBuf {
    workspace_store::save_item(root, &item).expect("the fixture item is written")
}

/// A workspace with a `base` global, an "Alpha" environment overriding `base`, and a
/// "Pets" collection holding `path` and one request to `{{base}}/{{path}}`.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    let globals = serde_json::to_value(vec![variable("base", "https://globals.example.test")])
        .expect("the globals serialize");
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Writes workspace",
        "format_version": "1.0",
        "variables": globals,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the workspace file is written");
    save(
        &root,
        Item {
            body: ItemBody::Environment(Environment {
                id: ItemId::new(),
                name: "Alpha".to_string(),
                variables: vec![variable("base", "https://alpha.example.test")],
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from("environments"),
            path: None,
        },
    );
    let request = HttpRequest {
        id: ItemId::new(),
        name: REQUEST_NAME.to_string(),
        method: Method::Get,
        url: "{{base}}/{{path}}".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    };
    let collection_file = save(
        &root,
        Item {
            body: ItemBody::Collection(Collection {
                id: ItemId::new(),
                name: "Pets".to_string(),
                description: None,
                auth: Auth::Inherit,
                variables: vec![variable("path", "pets")],
                order: vec![request.id],
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from("collections"),
            path: None,
        },
    );
    let collection_dir = collection_file
        .parent()
        .expect("a collection file lives in its own directory")
        .to_path_buf();
    save(
        &root,
        Item {
            body: ItemBody::Request(Request::Http(request)),
            dir: collection_dir,
            path: None,
        },
    );
    Fixture {
        _directory: directory,
        root,
    }
}

/// A second folder with a byte-for-byte copy of `fixture` (same ids).
fn copy_of(fixture: &Fixture) -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    copy_tree(&fixture.root, &root);
    Fixture {
        _directory: directory,
        root,
    }
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("the fixture folder reads") {
        let entry = entry.expect("the entry reads");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).expect("the folder is created");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file copies");
        }
    }
}

/// Every file under `root` with its text, sorted by path.
fn files(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut found = BTreeMap::new();
    collect(root, &mut found);
    found
}

fn collect(directory: &Path, found: &mut BTreeMap<PathBuf, String>) {
    for entry in std::fs::read_dir(directory).expect("the folder reads") {
        let path = entry.expect("the entry reads").path();
        if path.is_dir() {
            collect(&path, found);
        } else {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            found.insert(path, text);
        }
    }
}

fn boot<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    open_folder(cx, &workspace, fixture);
    (workspace, cx)
}

fn open_folder(cx: &mut VisualTestContext, workspace: &Entity<Workspace>, fixture: &Fixture) {
    let opener = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    let root = fixture.root.clone();
    cx.update(|_, cx| opener.update(cx, |opener, cx| opener.open_folder(root, cx)));
    cx.run_until_parked();
    draw_window(cx);
}

fn select_environment(cx: &mut VisualTestContext, name: &str) {
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input(name);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
}

fn active_editor(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<RequestEditor> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
        })
    })
    .expect("the active tab is a request editor")
}

fn open_request(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
) -> Entity<RequestEditor> {
    cx.dispatch_action(ToggleQuickOpen);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input(REQUEST_NAME);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
    active_editor(workspace, cx)
}

fn writes_of(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> VariableWrites {
    cx.read(|cx| {
        editor
            .read(cx)
            .url_bar()
            .read(cx)
            .hover()
            .read(cx)
            .variable_writes()
            .clone()
    })
}

/// Land one shared-value edit through `writes`, as the popover's field does.
fn set(
    cx: &mut VisualTestContext,
    writes: &VariableWrites,
    scope: ScopeKind,
    name: &str,
    value: &str,
) {
    let write = writes.write.clone().expect("the bound editor has a writer");
    let edit = VariableEdit {
        scope,
        name: name.to_string(),
        change: EditChange::Set {
            value: value.to_string(),
            local: false,
        },
    };
    cx.update(|window, cx| write(edit, window, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn wire_each_scope_kind_writes_its_own_file(cx: &mut TestAppContext) {
    let folder = fixture();
    let (workspace, cx) = boot(cx, &folder);
    select_environment(cx, "Alpha");
    let editor = open_request(cx, &workspace);
    let writes = writes_of(&editor, cx);
    let names: Vec<(ScopeKind, String)> = writes.scope_names.clone();
    assert!(
        names.contains(&(ScopeKind::Environment, "Alpha".to_string())),
        "{names:?}"
    );
    assert!(
        names.contains(&(ScopeKind::Global, "Globals".to_string())),
        "{names:?}"
    );
    assert!(
        names.contains(&(ScopeKind::Collection, "Pets".to_string())),
        "{names:?}"
    );

    set(
        cx,
        &writes,
        ScopeKind::Environment,
        "base",
        "https://env-edit.example.test",
    );
    set(
        cx,
        &writes,
        ScopeKind::Global,
        "base",
        "https://global-edit.example.test",
    );
    set(
        cx,
        &writes,
        ScopeKind::Collection,
        "path",
        "collection-edit",
    );

    let on_disk = files(&folder.root);
    let holding = |needle: &str| -> Vec<String> {
        on_disk
            .iter()
            .filter(|(_, text)| text.contains(needle))
            .map(|(path, _)| {
                path.strip_prefix(&folder.root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    };
    let environment = holding("env-edit.example.test");
    assert_eq!(environment.len(), 1, "{environment:?}");
    assert!(
        environment[0].starts_with("environments/"),
        "{environment:?}"
    );
    assert_eq!(
        holding("global-edit.example.test"),
        vec!["pustman.json".to_string()]
    );
    let collection = holding("collection-edit");
    assert_eq!(collection.len(), 1, "{collection:?}");
    assert!(collection[0].ends_with("collection.json"), "{collection:?}");
}

#[gpui::test]
fn wire_a_tab_from_another_folder_gets_no_writer(cx: &mut TestAppContext) {
    let folder_a = fixture();
    let folder_b = copy_of(&folder_a);
    let (workspace, cx) = boot(cx, &folder_a);
    select_environment(cx, "Alpha");
    let stale = open_request(cx, &workspace);
    assert!(
        writes_of(&stale, cx).write.is_some(),
        "bound to folder A, the tab can write"
    );
    let a_before = files(&folder_a.root);

    open_folder(cx, &workspace, &folder_b);
    select_environment(cx, "Alpha");

    let stale_writes = writes_of(&stale, cx);
    assert!(
        stale_writes.write.is_none() && stale_writes.scope_names.is_empty(),
        "the tab left from folder A is unbound here: it gets no writer"
    );

    // A tab of folder B writes folder B — never folder A.
    cx.dispatch_action(pustman_actions::workspace::NewRequest);
    cx.run_until_parked();
    cx.simulate_input("{{base}}/x");
    let fresh = active_editor(&workspace, cx);
    let fresh_writes = writes_of(&fresh, cx);
    set(
        cx,
        &fresh_writes,
        ScopeKind::Environment,
        "base",
        "https://b-edit.example.test",
    );

    assert_eq!(files(&folder_a.root), a_before, "folder A is untouched");
    assert!(
        files(&folder_b.root)
            .values()
            .any(|text| text.contains("b-edit.example.test")),
        "folder B's environment took the edit"
    );
}
