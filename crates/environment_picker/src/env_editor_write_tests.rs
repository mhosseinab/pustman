//! One variable written from the `{{var}}` popover (S48u6): an edit lands in the scope's
//! file — or in its local value — through the editor's own save, and an undefined name
//! is added to the active environment or to the globals. A secret is never written.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{TestAppContext, VisualTestContext};
use storage::LocalStore;
use ui::test_support::init_test;
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{ValueLayer, VariableChange, VariableWrite, WriteScope, write_variable};
use crate::env_editor::EditTarget;

const WORKSPACE_ID: &str = "workspace-write";
const FAKE_SECRET: &str = "token-REDACTED-example";

fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: None,
        extra: Default::default(),
    }
}

fn disabled(mut variable: Variable) -> Variable {
    variable.enabled = false;
    variable
}

fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace folder with `pustman.json` and a "Development" environment holding
/// `base_url` and a secret `api_key` (no value: secrets keep none in the file).
fn fixture(root: &Path) -> (Item, Item) {
    let workspace_item = Item {
        body: ItemBody::Workspace(WorkspaceModel {
            id: ItemId::new(),
            name: "Orbit".to_string(),
            format_version: workspace_store::FORMAT_VERSION.to_string(),
            extra: Default::default(),
        }),
        dir: PathBuf::new(),
        path: Some(PathBuf::from("pustman.json")),
    };
    workspace_store::save_item(root, &workspace_item).expect("pustman.json is written");
    let mut environment_item = Item {
        body: ItemBody::Environment(Environment {
            id: ItemId::new(),
            name: "Development".to_string(),
            variables: vec![
                variable(
                    "base_url",
                    Some("https://api.dev.example.test"),
                    VariableKind::Default,
                ),
                variable("api_key", None, VariableKind::Secret),
                disabled(variable("limit", Some("50"), VariableKind::Default)),
                disabled(variable("old_token", None, VariableKind::Secret)),
            ],
            extra: Default::default(),
        }),
        dir: PathBuf::from("environments"),
        path: None,
    };
    let written =
        workspace_store::save_item(root, &environment_item).expect("the environment is written");
    environment_item.path = Some(written);
    (workspace_item, environment_item)
}

fn scope(root: &Path, item: Item, store: Arc<Mutex<LocalStore>>) -> WriteScope {
    WriteScope {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target: EditTarget::new(item).expect("the item carries variables"),
        store,
    }
}

/// The variables of the item `id` as the folder holds them now.
fn variables_on_disk(root: &Path, item: &Item) -> Vec<Variable> {
    let snapshot = workspace_store::open(root).expect("the workspace reads");
    let id = item.id().expect("the fixture item has an id");
    let current = snapshot.items.get(&id).expect("the item is on disk");
    EditTarget::new(current.clone())
        .expect("the item carries variables")
        .variables()
        .to_vec()
}

fn run(
    cx: &mut VisualTestContext,
    workspace: &gpui::Entity<Workspace>,
    scope: WriteScope,
    write: VariableWrite,
) -> bool {
    let weak = workspace.downgrade();
    let landed = Rc::new(Cell::new(None));
    cx.update(|_window, cx| {
        let task = write_variable(scope, write, weak, cx);
        let landed = Rc::clone(&landed);
        cx.spawn(async move |_| landed.set(Some(task.await)))
            .detach();
    });
    cx.run_until_parked();
    landed.get().expect("the write resolves")
}

fn window(cx: &mut TestAppContext) -> (gpui::Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        workspace::init(cx);
    });
    cx.add_window_view(Workspace::new)
}

#[gpui::test]
fn test_an_edited_value_is_saved_into_the_environment_file(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);

    let landed = run(
        cx,
        &workspace,
        scope(&root, environment.clone(), store),
        VariableWrite {
            key: "base_url".to_string(),
            change: VariableChange::Edit {
                value: "https://api.staging.example.test".to_string(),
                layer: ValueLayer::Shared,
            },
        },
    );

    assert!(landed, "the write lands");
    let base_url = variables_on_disk(&root, &environment)
        .into_iter()
        .find(|variable| variable.key == "base_url")
        .expect("base_url is still there");
    assert_eq!(
        base_url.value.as_deref(),
        Some("https://api.staging.example.test"),
        "the edit is in the environment's own file"
    );
}

#[gpui::test]
fn test_an_edit_of_a_local_value_goes_to_the_local_store_not_the_file(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let db = directory.path().join("pustman.db");
    let store = open_store(&db);
    let (workspace, cx) = window(cx);

    let landed = run(
        cx,
        &workspace,
        scope(&root, environment.clone(), store),
        VariableWrite {
            key: "base_url".to_string(),
            change: VariableChange::Edit {
                value: "http://localhost:8080".to_string(),
                layer: ValueLayer::Local,
            },
        },
    );

    assert!(landed);
    let fresh = LocalStore::open(&db).expect("the store reopens");
    let locals = fresh
        .local_values()
        .list(WORKSPACE_ID)
        .expect("the local values read");
    assert!(
        locals
            .iter()
            .any(|local| local.key == "base_url" && local.value == "http://localhost:8080"),
        "the local value is stored on this machine"
    );
    let base_url = variables_on_disk(&root, &environment)
        .into_iter()
        .find(|variable| variable.key == "base_url")
        .expect("base_url is still there");
    assert_eq!(
        base_url.value.as_deref(),
        Some("https://api.dev.example.test"),
        "the shared value is untouched"
    );
}

#[gpui::test]
fn test_an_undefined_name_is_added_to_the_environment_and_to_the_globals(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (workspace_item, environment) = fixture(&root);
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);

    let to_environment = run(
        cx,
        &workspace,
        scope(&root, environment.clone(), Arc::clone(&store)),
        VariableWrite {
            key: "page_size".to_string(),
            change: VariableChange::Add {
                value: String::new(),
            },
        },
    );
    let to_globals = run(
        cx,
        &workspace,
        scope(&root, workspace_item.clone(), store),
        VariableWrite {
            key: "region".to_string(),
            change: VariableChange::Add {
                value: String::new(),
            },
        },
    );

    assert!(to_environment && to_globals);
    let environment_variables = variables_on_disk(&root, &environment);
    let added = environment_variables
        .iter()
        .find(|variable| variable.key == "page_size")
        .expect("page_size is added to Development");
    assert_eq!(added.kind, VariableKind::Default);
    assert!(added.enabled);
    assert!(
        variables_on_disk(&root, &workspace_item)
            .iter()
            .any(|variable| variable.key == "region"),
        "region is added to the globals in pustman.json"
    );
}

#[gpui::test]
fn test_a_write_aimed_at_a_secret_is_refused_and_writes_nothing(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let file = root.join(environment.path.clone().expect("the fixture was written"));
    let before = std::fs::read(&file).expect("the file reads");
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);

    let landed = run(
        cx,
        &workspace,
        scope(&root, environment, store),
        VariableWrite {
            key: "api_key".to_string(),
            change: VariableChange::Edit {
                value: FAKE_SECRET.to_string(),
                layer: ValueLayer::Shared,
            },
        },
    );

    assert!(!landed, "a secret is never written from outside the editor");
    let after = std::fs::read(&file).expect("the file reads");
    assert_eq!(before, after, "the file is untouched");
    assert!(
        !String::from_utf8_lossy(&after).contains(FAKE_SECRET),
        "the secret never reaches the file"
    );
}

/// "Add to <scope>" for `key` against the fixture: whether it landed, and the
/// environment file's bytes before and after.
fn add_existing(key: &str, cx: &mut TestAppContext) -> (bool, Vec<u8>, Vec<u8>) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let file = root.join(environment.path.clone().expect("the fixture was written"));
    let before = std::fs::read(&file).expect("the file reads");
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);
    let landed = run(
        cx,
        &workspace,
        scope(&root, environment, store),
        VariableWrite {
            key: key.to_string(),
            change: VariableChange::Add {
                value: String::new(),
            },
        },
    );
    let after = std::fs::read(&file).expect("the file reads");
    (landed, before, after)
}

#[gpui::test]
fn test_add_never_overwrites_a_disabled_variable(cx: &mut TestAppContext) {
    let (landed, before, after) = add_existing("limit", cx);
    assert!(!landed, "adding a name the scope already holds is refused");
    assert_eq!(before, after, "the disabled variable's value is untouched");
}

#[gpui::test]
fn test_add_never_overwrites_a_disabled_secret(cx: &mut TestAppContext) {
    let (landed, before, after) = add_existing("old_token", cx);
    assert!(!landed, "adding over a disabled secret is refused");
    assert_eq!(before, after, "the file is untouched");
}

#[gpui::test]
fn test_add_never_overwrites_an_enabled_variable(cx: &mut TestAppContext) {
    let (landed, before, after) = add_existing("base_url", cx);
    assert!(!landed, "adding a name that already resolves is refused");
    assert_eq!(before, after, "base_url keeps its value");
}

#[gpui::test]
fn test_a_refused_add_says_the_variable_is_disabled(_cx: &mut TestAppContext) {
    assert_eq!(
        super::existing_refusal("limit", "Development", false, VariableKind::Default),
        "limit exists in Development but is disabled — enable it in the environment editor."
    );
    assert_eq!(
        super::existing_refusal("old_token", "Development", false, VariableKind::Secret),
        "old_token exists in Development as a disabled secret — enable it and set its value in the environment editor."
    );
    assert_eq!(
        super::existing_refusal("base_url", "Development", true, VariableKind::Default),
        "base_url already exists in Development — edit its value instead."
    );
}

#[gpui::test]
fn test_an_edit_of_a_name_the_scope_no_longer_holds_is_refused(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let file = root.join(environment.path.clone().expect("the fixture was written"));
    let before = std::fs::read(&file).expect("the file reads");
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);

    let landed = run(
        cx,
        &workspace,
        scope(&root, environment, store),
        VariableWrite {
            key: "limit".to_string(),
            change: VariableChange::Edit {
                value: "100".to_string(),
                layer: ValueLayer::Shared,
            },
        },
    );

    assert!(!landed, "an edit never revives a disabled variable");
    assert_eq!(before, std::fs::read(&file).expect("the file reads"));
}

/// A workspace window with the variables editor open on `item`'s scope.
fn window_with_editor<'a>(
    root: &Path,
    item: Item,
    store: Arc<Mutex<LocalStore>>,
    cx: &'a mut TestAppContext,
) -> (
    gpui::Entity<Workspace>,
    gpui::Entity<crate::env_editor::EnvEditor>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    let inputs = crate::env_editor::EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target: EditTarget::new(item).expect("the item carries variables"),
        local_values: Vec::new(),
        store,
    };
    cx.update(|_window, cx| crate::env_editor::install_inputs(cx, inputs));
    cx.dispatch_action(crate::env_editor::Open);
    cx.run_until_parked();
    let editor = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace
                    .modal_layer()
                    .active_modal::<crate::env_editor::EnvEditor>()
            })
        })
        .expect("env_editor::Open opens the editor");
    (workspace, editor, cx)
}

#[gpui::test]
fn test_a_local_write_refreshes_the_open_editor_so_its_save_cannot_clobber_it(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, editor, cx) =
        window_with_editor(&root, environment.clone(), Arc::clone(&store), cx);

    let landed = run(
        cx,
        &workspace,
        scope(&root, environment, store),
        VariableWrite {
            key: "base_url".to_string(),
            change: VariableChange::Edit {
                value: "http://localhost:9000".to_string(),
                layer: ValueLayer::Local,
            },
        },
    );

    assert!(landed);
    let shown = cx.read(|cx| {
        editor
            .read(cx)
            .local_value_field("base_url")
            .map(|field| field.read(cx).text())
    });
    assert_eq!(
        shown.as_deref(),
        Some("http://localhost:9000"),
        "the open editor shows the value the popover saved, so its Save keeps it"
    );
}

#[gpui::test]
fn test_two_quick_adds_to_one_scope_both_land(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().join("orbit");
    std::fs::create_dir_all(&root).expect("the folder is created");
    let (_workspace_item, environment) = fixture(&root);
    let store = open_store(&directory.path().join("pustman.db"));
    let (workspace, cx) = window(cx);

    let weak = workspace.downgrade();
    let results = Rc::new(std::cell::RefCell::new(Vec::new()));
    cx.update(|_window, cx| {
        for key in ["first", "second"] {
            let task = write_variable(
                scope(&root, environment.clone(), Arc::clone(&store)),
                VariableWrite {
                    key: key.to_string(),
                    change: VariableChange::Add {
                        value: String::new(),
                    },
                },
                weak.clone(),
                cx,
            );
            let results = Rc::clone(&results);
            cx.spawn(async move |_| {
                let landed = task.await;
                results.borrow_mut().push(landed);
            })
            .detach();
        }
    });
    cx.run_until_parked();

    assert_eq!(
        results.borrow().as_slice(),
        &[true, true],
        "both writes land"
    );
    let keys: Vec<String> = variables_on_disk(&root, &environment)
        .into_iter()
        .map(|variable| variable.key)
        .collect();
    assert!(
        keys.contains(&"first".to_string()) && keys.contains(&"second".to_string()),
        "neither write is lost to the other: {keys:?}"
    );
}
