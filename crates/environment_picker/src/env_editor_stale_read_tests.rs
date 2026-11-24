//! What the editor may never do: save from a copy it could not re-read. A file left
//! behind by a merge conflict and a file a teammate deleted both leave the editor without
//! a current copy of the scope — writing the wire's last rows over them would overwrite
//! the conflict or resurrect the file, so the editor says why and refuses to save. An
//! item that has never been written has no file to protect and saves normally.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

/// The workspace every fixture's rows and local values are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

const ALPHA_KEY: &str = "alpha";
const BETA_KEY: &str = "beta";
const ALPHA_FIRST: &str = "https://one.example.test";
const BETA_FIRST: &str = "https://two.example.test";
const ALPHA_EDIT: &str = "https://one-edited.example.test";
const BETA_EDIT: &str = "https://two-edited.example.test";

/// The file a merge left behind: conflict markers around both halves, which
/// `workspace_store::open` reports as a problem instead of an item.
const CONFLICTED_FILE: &str = r#"{
  "environment": {
<<<<<<< HEAD
    "id": "01HZZZZZZZZZZZZZZZZZZZZZZZZ",
    "name": "Staging",
    "values": []
=======
    "id": "01HZZZZZZZZZZZZZZZZZZZZZZZZ",
    "name": "Staging (theirs)",
    "values": []
>>>>>>> teammate
  }
}
"#;

/// One ordinary variable of the fixture environment.
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

/// The environment the wire last saw — the copy the editor must never save blindly.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![
            variable(ALPHA_KEY, ALPHA_FIRST),
            variable(BETA_KEY, BETA_FIRST),
        ],
        extra: BTreeMap::new(),
    }
}

/// The workspace item the environment's shared values are saved to.
fn environment_item(environment: &Environment) -> Item {
    Item {
        body: ItemBody::Environment(environment.clone()),
        dir: PathBuf::from("environments"),
        path: Some(PathBuf::from("environments/staging.environment.json")),
    }
}

/// The workspace item of an environment that has never been written (module docs).
fn unwritten_item(environment: &Environment) -> Item {
    Item {
        body: ItemBody::Environment(environment.clone()),
        dir: PathBuf::from("environments"),
        path: None,
    }
}

/// A workspace folder holding `pustman.json`, and no environment file yet.
fn workspace_root(root: &Path) {
    let workspace_file = api_model::Workspace {
        id: ItemId::new(),
        name: "Fixture workspace".to_string(),
        format_version: "1.0".to_string(),
        extra: BTreeMap::new(),
    };
    std::fs::create_dir_all(root.join("environments")).expect("the environments folder is created");
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_vec_pretty(&workspace_file).expect("the workspace file serializes"),
    )
    .expect("pustman.json is written");
}

/// The local store the editor saves local values into.
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The inputs one open round runs with: the wire's last copy of the scope, item and all.
fn inputs(root: &Path, store: Arc<Mutex<LocalStore>>, item: Item) -> EnvEditorInputs {
    let target = EditTarget::new(item).expect("an environment is editable");
    EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target,
        local_values: Vec::new(),
        store,
    }
}

/// A workspace window with this crate wired up, with the Save keystroke bound over the
/// editor's key context.
fn editor_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        cx.bind_keys([KeyBinding::new("ctrl-s", Save, Some("EnvEditor"))]);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// One open round: dispatch `env_editor::Open` over the installed inputs and hand the
/// editor back.
fn open_editor(cx: &mut VisualTestContext, workspace: &Entity<Workspace>) -> Entity<EnvEditor> {
    cx.dispatch_action(Open);
    cx.run_until_parked();
    draw_window(cx);
    let editor = editor_of(workspace, cx).expect("dispatching env_editor::Open opens the editor");
    cx.run_until_parked();
    editor
}

/// The editor the workspace's modal layer is showing, if it is open.
fn editor_of(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<Entity<EnvEditor>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<EnvEditor>()
        })
    })
}

/// Try to save the way the user does: edit a row, then the Save keystroke.
fn edit_and_save(editor: &Entity<EnvEditor>, cx: &mut VisualTestContext) {
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(ALPHA_KEY, ALPHA_EDIT, cx));
        })
    });
    draw_window(cx);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);
}

/// The inline message the editor is showing.
fn editor_error(editor: &Entity<EnvEditor>, cx: &VisualTestContext) -> String {
    cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()))
        .expect("the editor says why")
        .to_string()
}

/// The variables of the environment file as it sits on disk.
fn file_variables(root: &Path) -> Vec<(String, Option<String>)> {
    let bytes = std::fs::read(root.join("environments/staging.environment.json"))
        .expect("the environment file reads");
    let file: serde_json::Value = serde_json::from_slice(&bytes).expect("the file is JSON");
    file["variables"]
        .as_array()
        .expect("the file carries its variables")
        .iter()
        .map(|variable| {
            (
                variable["key"]
                    .as_str()
                    .expect("every row has a key")
                    .to_string(),
                variable["value"].as_str().map(str::to_string),
            )
        })
        .collect()
}

#[gpui::test]
fn conflict_marked_file_refuses_save_and_says_why(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_root(root.path());
    let conflicted = root.path().join("environments/staging.environment.json");
    std::fs::write(&conflicted, CONFLICTED_FILE).expect("the conflicted file is written");
    let before = std::fs::read(&conflicted).expect("the conflicted file reads");
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    cx.update(|_window, cx| {
        install_inputs(
            cx,
            inputs(root.path(), store, environment_item(&environment)),
        );
    });
    let editor = open_editor(cx, &workspace);

    edit_and_save(&editor, cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a refused save keeps the editor open on its rows"
    );
    let error = editor_error(&editor, cx);
    assert!(
        error.contains("merge-conflict"),
        "the refusal says why (got: {error})"
    );
    assert_eq!(
        std::fs::read(&conflicted).expect("the conflicted file reads"),
        before,
        "the conflicted file is left exactly as the merge left it"
    );
}

#[gpui::test]
fn deleted_file_refuses_save_and_stays_deleted(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_root(root.path());
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    // The item the wire last saw had a file; a teammate then deleted it.
    cx.update(|_window, cx| {
        install_inputs(
            cx,
            inputs(root.path(), store, environment_item(&environment)),
        );
    });
    let editor = open_editor(cx, &workspace);

    edit_and_save(&editor, cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a refused save keeps the editor open on its rows"
    );
    let error = editor_error(&editor, cx);
    assert!(
        error.contains("gone from disk"),
        "the refusal says why (got: {error})"
    );
    assert!(
        !root
            .path()
            .join("environments/staging.environment.json")
            .exists(),
        "a deleted file stays deleted — the editor does not resurrect it"
    );
}

#[gpui::test]
fn never_written_item_can_be_saved_twice(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_root(root.path());
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    // An environment that has never been written: no file to be stale about.
    cx.update(|_window, cx| {
        install_inputs(cx, inputs(root.path(), store, unwritten_item(&environment)));
    });

    let editor = open_editor(cx, &workspace);
    edit_and_save(&editor, cx);
    assert!(
        editor_of(&workspace, cx).is_none(),
        "a never-written item saves like any other"
    );

    // The second session sees the file the first save created.
    let editor = open_editor(cx, &workspace);
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(BETA_KEY, BETA_EDIT, cx));
        })
    });
    draw_window(cx);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        file_variables(root.path()),
        vec![
            (ALPHA_KEY.to_string(), Some(ALPHA_EDIT.to_string())),
            (BETA_KEY.to_string(), Some(BETA_EDIT.to_string())),
        ],
        "both saves land — the second one does not refuse over the file the first created"
    );
}
