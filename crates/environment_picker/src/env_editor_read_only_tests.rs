//! A read-only workspace refuses the variables editor's save before anything is written
//! anywhere (REQ-WSP-04): no local values, no keychain entries, no file — the commit
//! point never moves — and the user is told why instead of losing the press. The fields
//! take no typing either. Driven the way a user drives it: the real `OpenWorkspace`
//! action over a newer major `format_version` folder (S41c's fixture shape), a shared
//! value, a local value and a secret each edited, then the Save keystroke. The keychain
//! double is a `secrets::MemorySecretStore`, never the real one
//! (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind};
use gpui::{Entity, Focusable, KeyBinding, TestAppContext, VisualTestContext};
use pustman_actions::workspace::OpenWorkspace;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{LocalStore, LocalValue};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace::read_only::{SAVE_REFUSED_MESSAGE, is_read_only};
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};
use crate::secret_values::{SecretInputs, install_inputs as install_secret_inputs};

/// The workspace every fixture's rows and local values are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture's rows and edits — obviously fake (`.agents/rules/tests.md`).
const ALPHA_KEY: &str = "alpha";
const BETA_KEY: &str = "beta";
const TOKEN_KEY: &str = "token";
const ALPHA_SHARED: &str = "https://one.example.test";
const BETA_SHARED: &str = "https://two.example.test";
const ALPHA_EDIT: &str = "https://one-edited.example.test";
const BETA_LOCAL: &str = "local-REDACTED-example";
const BETA_LOCAL_EDIT: &str = "local-REDACTED-edited";
const TOKEN_SET: &str = "token-REDACTED-old";
const TOKEN_EDIT: &str = "token-REDACTED-new";

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

/// The environment the test edits: two ordinary variables and one secret.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![
            variable(ALPHA_KEY, ALPHA_SHARED),
            variable(BETA_KEY, BETA_SHARED),
            Variable {
                key: TOKEN_KEY.to_string(),
                value: None,
                kind: VariableKind::Secret,
                enabled: true,
                description: None,
                extra: BTreeMap::new(),
            },
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

/// A workspace folder whose `pustman.json` declares a newer major `format_version` than
/// this build writes (S41c's fixture shape), holding the environment's file.
fn read_only_workspace_with(root: &Path, environment: &Environment) {
    std::fs::create_dir_all(root.join("environments")).expect("the environments folder is created");
    let workspace_file = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Fixture workspace",
        "format_version": "2.0",
    });
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
/// editor's key context and the workspace wiring the read-only flag comes from.
fn editor_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        workspace::init(cx);
        crate::init(cx);
        cx.bind_keys([KeyBinding::new("ctrl-s", Save, Some("EnvEditor"))]);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// Open `root` as this window's workspace: the real `OpenWorkspace` action and its
/// folder dialog (the `read_only_tests.rs` shape).
fn open_workspace_folder(cx: &mut VisualTestContext, root: &Path) {
    cx.dispatch_action(OpenWorkspace);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "OpenWorkspace asks for the workspace folder in a dialog"
    );
    cx.simulate_path_prompt_response(|_| Some(vec![root.to_path_buf()]));
    cx.run_until_parked();
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

#[gpui::test]
fn read_only_workspace_env_save_writes_nothing(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    read_only_workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    // A previous session's local value for `beta` — what a save must leave alone.
    store
        .lock()
        .expect("the store lock is free")
        .local_values()
        .insert(&LocalValue {
            workspace_id: WORKSPACE_ID.to_string(),
            scope_id: EditTarget::new(environment_item(&environment))
                .expect("an environment is editable")
                .scope_id()
                .to_owned(),
            key: BETA_KEY.to_string(),
            value: BETA_LOCAL.to_string(),
        })
        .expect("the fixture local value is stored");
    // The keychain entry the secret row opened on — what a save must leave alone.
    let secrets = Arc::new(MemorySecretStore::new());
    let scope_id = EditTarget::new(environment_item(&environment))
        .expect("an environment is editable")
        .scope_id()
        .to_owned();
    let entry_key = SecretKey::new(WORKSPACE_ID, scope_id.clone(), TOKEN_KEY)
        .expect("the fixture key is valid");
    secrets
        .set(&entry_key, &SecretValue::new(TOKEN_SET))
        .expect("the fixture entry is set");

    let (workspace, cx) = editor_window(cx);
    open_workspace_folder(cx, root.path());
    assert!(
        cx.read(is_read_only),
        "the fixture workspace opens read-only"
    );
    cx.update(|_window, cx| {
        install_secret_inputs(
            cx,
            SecretInputs {
                store: Arc::clone(&secrets) as Arc<dyn SecretStore>,
            },
        );
    });
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), Arc::clone(&store), environment, Vec::new()),
    );

    // The fields take no typing: a keystroke into one changes nothing.
    let alpha = cx
        .read(|cx| editor.read_with(cx, |editor, _| editor.shared_value_field(ALPHA_KEY)))
        .expect("the alpha row keeps its shared field");
    cx.update(|window, cx| {
        let focus_handle = alpha.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input("!");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| alpha.read_with(cx, |field, _| field.text())),
        ALPHA_SHARED,
        "typing into a read-only field changes nothing"
    );

    // The user edits a shared value, a local value and a secret, then saves.
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_shared_value(ALPHA_KEY, ALPHA_EDIT, cx));
            assert!(editor.set_local_value(BETA_KEY, BETA_LOCAL_EDIT, cx));
            assert!(editor.set_secret_value(TOKEN_KEY, TOKEN_EDIT, cx));
        });
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a refused save keeps the editor open on the edits it did not lose"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the refusal says why");
    assert_eq!(
        error.to_string(),
        SAVE_REFUSED_MESSAGE,
        "the user sees the read-only message"
    );
    assert_eq!(
        file_variables(root.path()),
        vec![
            (ALPHA_KEY.to_string(), Some(ALPHA_SHARED.to_string())),
            (BETA_KEY.to_string(), Some(BETA_SHARED.to_string())),
            (TOKEN_KEY.to_string(), None),
        ],
        "the shared file is exactly as the save found it"
    );
    let locals = store
        .lock()
        .expect("the store lock is free")
        .local_values()
        .list(WORKSPACE_ID)
        .expect("the local values read");
    assert_eq!(
        locals,
        vec![LocalValue {
            workspace_id: WORKSPACE_ID.to_string(),
            scope_id,
            key: BETA_KEY.to_string(),
            value: BETA_LOCAL.to_string(),
        }],
        "the SQLite local values are exactly as the save found them"
    );
    let stored = secrets
        .get(&entry_key)
        .expect("the keychain entry is still there");
    assert_eq!(
        stored.expose(),
        TOKEN_SET,
        "the keychain is exactly as the save found it"
    );
}
