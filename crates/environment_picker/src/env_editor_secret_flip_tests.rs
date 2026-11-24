//! A variable switched to Secret keeps no local value at all — its value belongs to the
//! OS keychain — so the save removes whatever local row its key still has in `storage`.
//! A row that survived the flip would keep a plaintext value the editor no longer shows
//! (REQ-VAR-SHARE-02).

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

/// The stale local value a previous session left in the store — obviously fake
/// (`.agents/rules/tests.md`).
const STALE_LOCAL_VALUE: &str = "local-REDACTED-example";

/// The fixture row: one ordinary variable whose value the test makes secret.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![Variable {
            key: "api_token".to_string(),
            value: Some("shared-REDACTED-example".to_string()),
            kind: VariableKind::Default,
            enabled: true,
            description: Some("the API's token".to_string()),
            extra: BTreeMap::new(),
        }],
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
    std::fs::write(
        root.join("environments/staging.environment.json"),
        serde_json::to_vec_pretty(environment).expect("the environment file serializes"),
    )
    .expect("the environment file is written");
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

#[gpui::test]
fn flipping_a_row_to_secret_removes_its_local_value(cx: &mut TestAppContext) {
    let environment = staging();
    let scope_id = environment.id.to_string();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    // A local value a previous session stored for the row.
    let stale = LocalValue {
        workspace_id: WORKSPACE_ID.to_string(),
        scope_id: scope_id.clone(),
        key: "api_token".to_string(),
        value: STALE_LOCAL_VALUE.to_string(),
    };
    store
        .lock()
        .expect("the store lock is healthy")
        .local_values()
        .insert(&stale)
        .expect("the local value persists");
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment, vec![stale]),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(
                editor.set_secret("api_token", true, window, cx),
                "the row flips to Secret"
            );
        })
    });
    draw_window(cx);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);

    let fresh = LocalStore::open(root.path().join("local.db")).expect("the store reopens");
    assert!(
        fresh
            .local_values()
            .list(WORKSPACE_ID)
            .expect("the local values read")
            .is_empty(),
        "the flipped row's plaintext local value is gone from storage"
    );
    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file was written");
    let file: serde_json::Value =
        serde_json::from_slice(&file_bytes).expect("the environment file is JSON");
    assert_eq!(
        file["variables"][0]["kind"].as_str(),
        Some("secret"),
        "the row is saved as a secret"
    );
    assert!(
        file["variables"][0].get("value").is_none(),
        "a secret records no value in the file (REQ-VAR-SHARE-02)"
    );
}
