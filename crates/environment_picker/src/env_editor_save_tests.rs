//! What a save may never do: write stale rows back over newer ones, write over a file a
//! teammate changed after this editor read it, or leave the shared file changed when the
//! local values could not be written. The file is the commit point (module docs) — a save
//! that cannot finish leaves it as it found it and says which part was saved.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use storage::{LocalStore, LocalValue};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

/// The workspace every fixture's rows and local values are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The values the fixtures start with and the edits each test types.
const ALPHA_KEY: &str = "alpha";
const BETA_KEY: &str = "beta";
const ALPHA_FIRST: &str = "https://one.example.test";
const BETA_FIRST: &str = "https://two.example.test";
const ALPHA_EDIT: &str = "https://one-edited.example.test";
const BETA_EDIT: &str = "https://two-edited.example.test";
const ALPHA_TEAMMATE: &str = "https://one-teammate.example.test";
const LOCAL_EDIT: &str = "local-REDACTED-example";

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

/// The environment the tests edit: two ordinary variables.
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

/// A workspace folder holding `pustman.json` and the fixture environment's file.
fn workspace_with(root: &Path, environment: &Environment) {
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
    write_environment_file(root, environment);
}

/// Write the environment's file — the fixture's first write, and a teammate's later one.
fn write_environment_file(root: &Path, environment: &Environment) {
    std::fs::write(
        root.join("environments/staging.environment.json"),
        serde_json::to_vec_pretty(environment).expect("the environment file serializes"),
    )
    .expect("the environment file is written");
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

/// The local store the editor saves local values into.
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The inputs one open round runs with.
fn inputs(
    root: &Path,
    store: Arc<Mutex<LocalStore>>,
    environment: Environment,
    local_values: Vec<LocalValue>,
) -> EnvEditorInputs {
    let target =
        EditTarget::new(environment_item(&environment)).expect("an environment is editable");
    EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target,
        local_values,
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

/// One open round: install `inputs`, dispatch `env_editor::Open`, hand the editor back.
fn open_editor(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
    inputs: EnvEditorInputs,
) -> Entity<EnvEditor> {
    cx.update(|_window, cx| install_inputs(cx, inputs));
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

/// Save the way the user does: the keystroke over the editor's key context.
fn save(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);
}

/// A store whose lock a panicking writer left poisoned: every write through it fails
/// (`response_viewer`'s `cookie_dialog_save_failure_tests` shape).
fn poison(store: &Arc<Mutex<LocalStore>>) {
    let poisoner = Arc::clone(store);
    let joined = std::thread::spawn(move || {
        let _held = poisoner.lock().expect("the store lock is free");
        panic!("poison the store lock for the test");
    })
    .join();
    assert!(
        joined.is_err(),
        "the poisoning thread panicked while holding the lock"
    );
}

#[gpui::test]
fn save_twice_keeps_both_edits(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);

    // First session: edit `alpha` and save.
    let editor = open_editor(
        cx,
        &workspace,
        inputs(
            root.path(),
            Arc::clone(&store),
            environment.clone(),
            Vec::new(),
        ),
    );
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(ALPHA_KEY, ALPHA_EDIT, cx));
        })
    });
    draw_window(cx);
    save(cx);
    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );

    // Second session over the same installed inputs: edit `beta` and save.
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment, Vec::new()),
    );
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(BETA_KEY, BETA_EDIT, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert_eq!(
        file_variables(root.path()),
        vec![
            (ALPHA_KEY.to_string(), Some(ALPHA_EDIT.to_string())),
            (BETA_KEY.to_string(), Some(BETA_EDIT.to_string())),
        ],
        "the second save keeps the first one's edit instead of reverting it"
    );
}

#[gpui::test]
fn changed_on_disk_refuses_save_and_says_so(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone(), Vec::new()),
    );

    // A teammate's change lands after this editor read the file.
    let mut teammate = environment;
    teammate.variables = vec![
        variable(ALPHA_KEY, ALPHA_TEAMMATE),
        variable(BETA_KEY, BETA_FIRST),
    ];
    write_environment_file(root.path(), &teammate);

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(ALPHA_KEY, ALPHA_EDIT, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a refused save keeps the editor open on the edits it did not lose"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the refusal says why");
    assert!(
        error.to_lowercase().contains("changed on disk"),
        "the refusal names what happened, not just that it failed (got: {error})"
    );
    assert_eq!(
        file_variables(root.path()),
        vec![
            (ALPHA_KEY.to_string(), Some(ALPHA_TEAMMATE.to_string())),
            (BETA_KEY.to_string(), Some(BETA_FIRST.to_string())),
        ],
        "the teammate's file is left exactly as they wrote it"
    );
}

#[gpui::test]
fn local_store_failure_leaves_the_shared_file_unchanged(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), Arc::clone(&store), environment, Vec::new()),
    );
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(ALPHA_KEY, ALPHA_EDIT, cx));
            assert!(editor.set_local_value(ALPHA_KEY, LOCAL_EDIT, cx));
        })
    });
    draw_window(cx);
    poison(&store);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a failed save keeps the editor open"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the failure is shown");
    assert!(
        error.contains("shared file is unchanged"),
        "the failure says which part was saved (got: {error})"
    );
    assert_eq!(
        file_variables(root.path()),
        vec![
            (ALPHA_KEY.to_string(), Some(ALPHA_FIRST.to_string())),
            (BETA_KEY.to_string(), Some(BETA_FIRST.to_string())),
        ],
        "the shared file is unchanged after a local-store failure"
    );
}
