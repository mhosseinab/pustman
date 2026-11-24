//! What a failed or refused save leaves behind (PRD REQ-VAR-SHARE-02): a refused (stale
//! or unreadable) save writes nothing at all — no keychain entry, no local value; a save
//! that fails after keychain writes restores the values it overwrote before reporting; a
//! failure partway through the local values never claims "Nothing was saved"; and a kind
//! flip round trip neither loses a stored value nor turns a cleared field into a silent
//! keep. Keychain doubles are `secrets` types under the same `SecretStore` contract —
//! never the real keychain (`.agents/rules/tests.md`) — and the local store is a real one
//! with a fault injected into it by hand (below).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{LocalStore, LocalValue};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::env_editor::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

use super::SecretInputs;

/// The workspace every fixture's rows are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture values — obviously fake (`.agents/rules/tests.md`).
const VALUE_TOKEN: &str = "token-REDACTED-old";
const VALUE_B: &str = "token-REDACTED-b";
const VALUE_NEW: &str = "token-REDACTED-new";
const LOCAL_OLD: &str = "local-REDACTED-old";

/// One secret fixture row.
fn secret_row(key: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: None,
        kind: VariableKind::Secret,
        enabled: true,
        description: Some("a secret".to_string()),
        extra: BTreeMap::new(),
    }
}

/// One ordinary fixture row.
fn default_row(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: Some("an ordinary variable".to_string()),
        extra: BTreeMap::new(),
    }
}

/// The environment the tests edit, out of `variables`.
fn staging(variables: Vec<Variable>) -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables,
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

/// A workspace folder holding `pustman.json` and the fixture environment's file, so a
/// save rewrites real files rather than creating them.
fn workspace_with(root: &Path, environment: &Environment) {
    let workspace_file = WorkspaceModel {
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

/// Rewrite `pustman.json` so `workspace_store::save_item` refuses every write in the
/// workspace: a newer major `format_version` is a read-only workspace (REQ-WSP-04).
fn write_future_workspace(root: &Path) {
    let workspace_file = WorkspaceModel {
        id: ItemId::new(),
        name: "Fixture workspace".to_string(),
        format_version: "2.0".to_string(),
        extra: BTreeMap::new(),
    };
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

/// Seed one local value the way a previous session would have left it.
fn seed_local(store: &Arc<Mutex<LocalStore>>, environment: &Environment, key: &str, value: &str) {
    store
        .lock()
        .expect("the store lock is healthy")
        .local_values()
        .insert(&LocalValue {
            workspace_id: WORKSPACE_ID.to_string(),
            scope_id: environment.id.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        })
        .expect("the local value persists");
}

/// The local values the store holds for the workspace, as rows.
fn local_rows(store: &Arc<Mutex<LocalStore>>) -> Vec<(String, String)> {
    store
        .lock()
        .expect("the store lock is healthy")
        .local_values()
        .list(WORKSPACE_ID)
        .expect("the local values read")
        .into_iter()
        .map(|value| (value.key, value.value))
        .collect()
}

/// The inputs one open round runs with: the fixture environment over `root`.
fn inputs(root: &Path, store: Arc<Mutex<LocalStore>>, environment: Environment) -> EnvEditorInputs {
    let target =
        EditTarget::new(environment_item(&environment)).expect("an environment is editable");
    EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target,
        local_values: Vec::new(),
        store,
    }
}

/// A workspace window with this crate wired up to `secret_store`, drawn and active, with
/// the Save keystroke bound over the editor's key context.
fn editor_window(
    cx: &mut TestAppContext,
    secret_store: Arc<dyn SecretStore>,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        super::install_inputs(
            cx,
            SecretInputs {
                store: secret_store,
            },
        );
        cx.bind_keys([KeyBinding::new("ctrl-s", Save, Some("EnvEditor"))]);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// One open round: install `inputs`, dispatch `env_editor::Open`, and hand the editor
/// back once its rows are on screen and its keychain read has landed.
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

/// Save as the user saves it: the Save keystroke over the editor's key context.
fn save(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);
}

/// The message the editor shows for the last save.
fn editor_error(editor: &Entity<EnvEditor>, cx: &VisualTestContext) -> String {
    cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()))
        .expect("the failure is shown")
        .to_string()
}

/// The keychain key one fixture secret lives under.
fn secret_key(environment: &Environment, variable_key: &str) -> SecretKey {
    SecretKey::new(WORKSPACE_ID, environment.id.to_string(), variable_key)
        .expect("the fixture key is representable in the keychain")
}

/// Seed `value` under `variable_key` the way a previous session would have left it.
fn seed(store: &dyn SecretStore, environment: &Environment, variable_key: &str, value: &str) {
    store
        .set(
            &secret_key(environment, variable_key),
            &SecretValue::new(value),
        )
        .expect("the fixture secret is seeded");
}

/// The value stored under `variable_key`, or [`None`] when there is no entry.
fn stored(
    store: &dyn SecretStore,
    environment: &Environment,
    variable_key: &str,
) -> Option<String> {
    store
        .get(&secret_key(environment, variable_key))
        .ok()
        .map(|value| value.expose().to_string())
}

#[gpui::test]
fn a_stale_save_leaves_local_values_and_keychain_entries_unchanged(cx: &mut TestAppContext) {
    let environment = staging(vec![
        secret_row("api_token"),
        default_row("base_url", "https://staging.example.test"),
    ]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    seed_local(&store, &environment, "base_url", LOCAL_OLD);
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store.clone(), environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret_value("api_token", VALUE_NEW, cx));
            assert!(editor.set_local_value("base_url", "https://edited.example.test", cx));
        })
    });
    // A teammate's change lands after this editor read the file.
    let mut teammate = environment.clone();
    teammate.name = "Teammate's staging".to_string();
    std::fs::write(
        root.path().join("environments/staging.environment.json"),
        serde_json::to_vec_pretty(&teammate).expect("the environment file serializes"),
    )
    .expect("the environment file is written");
    draw_window(cx);
    save(cx);

    let error = editor_error(&editor, cx);
    assert!(
        error.to_lowercase().contains("changed on disk"),
        "the refusal names what happened (got: {error})"
    );
    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        Some(VALUE_TOKEN.to_string()),
        "a refused save leaves the keychain entry exactly as it was"
    );
    assert_eq!(
        local_rows(&store),
        vec![("base_url".to_string(), LOCAL_OLD.to_string())],
        "a refused save leaves the local values exactly as they were"
    );
}

#[gpui::test]
fn a_failed_file_write_restores_the_overwritten_secret_value(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("b"), secret_row("token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "b", VALUE_B);
    seed(&*secrets, &environment, "token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    // The rotation: `b` grows into the `token` name with a new value, overwriting the
    // value `token` held; the old `token` row is renamed away and cleared.
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_key("b", "token", cx));
            assert!(editor.set_secret_value("b", VALUE_NEW, cx));
            assert!(editor.set_key("token", "token_old", cx));
            assert!(editor.set_secret_value("token", "", cx));
        })
    });
    write_future_workspace(root.path());
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save that could not write the file keeps the editor open"
    );
    let error = editor_error(&editor, cx);
    assert!(
        error.contains("shared file is unchanged"),
        "the message says the shared file is unchanged (got: {error})"
    );
    assert!(
        error.contains("restored"),
        "the message says the overwritten value was restored (got: {error})"
    );
    assert_eq!(
        stored(&*secrets, &environment, "token"),
        Some(VALUE_TOKEN.to_string()),
        "the value the failed save overwrote is readable again"
    );
}

#[gpui::test]
fn a_failure_partway_through_the_local_values_does_not_claim_nothing_was_saved(
    cx: &mut TestAppContext,
) {
    let environment = staging(vec![
        default_row("alpha", "one"),
        default_row("beta", "two"),
        default_row("gamma", "three"),
    ]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store_path = root.path().join("local.db");
    let store = open_store(&store_path);
    // A fault in the store itself: the write for `beta` is aborted by a trigger, so the
    // save's local writes land one by one and the second of the three fails.
    let fault = rusqlite::Connection::open(&store_path).expect("the fault connection opens");
    fault
        .execute_batch(
            "CREATE TRIGGER fail_beta BEFORE INSERT ON local_values
             WHEN NEW.key = 'beta'
             BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;",
        )
        .expect("the fault is installed");
    drop(fault);
    let secrets = Arc::new(MemorySecretStore::new());
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store.clone(), environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_local_value("alpha", "one-edited", cx));
            assert!(editor.set_local_value("beta", "two-edited", cx));
            assert!(editor.set_local_value("gamma", "three-edited", cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save whose local writes failed keeps the editor open"
    );
    let error = editor_error(&editor, cx);
    assert!(
        !error.contains("Nothing was saved"),
        "a failure after one write landed never claims nothing was saved (got: {error})"
    );
    assert!(
        error.contains("shared file is unchanged"),
        "the message says the shared file is unchanged (got: {error})"
    );
    assert_eq!(
        local_rows(&store),
        vec![("alpha".to_string(), "one-edited".to_string())],
        "the first of the three writes landed before the failure"
    );
}

#[gpui::test]
fn a_flip_round_trip_then_a_rename_moves_the_stored_value(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
            assert!(editor.set_secret("api_token", true, window, cx));
            assert!(editor.set_key("api_token", "api_token2", cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert_eq!(
        stored(&*secrets, &environment, "api_token2"),
        Some(VALUE_TOKEN.to_string()),
        "the flip round trip restores the field and the rename moves the stored value"
    );
    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        None,
        "the old key is cleaned up behind the move"
    );
}

#[gpui::test]
fn clearing_a_field_the_user_typed_into_removes_the_entry(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
            assert!(editor.set_secret("api_token", true, window, cx));
            // The user types a new value into the restored field …
            assert!(editor.set_secret_value("api_token", VALUE_NEW, cx));
            // … and then clears it again: that is a removal, not "leave it be".
            assert!(editor.set_secret_value("api_token", "", cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );
    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        None,
        "a field the user typed into and cleared removes the stored value"
    );
}
