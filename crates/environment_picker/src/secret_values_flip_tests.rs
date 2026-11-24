//! A typed secret survives a kind flip, and the rollback fix 2 added is pinned where it
//! can go wrong (PRD REQ-VAR-SHARE-02): a keychain write that fails after an earlier one
//! landed restores the earlier one, and a restore that itself fails names the keys that
//! may now hold the new value — never a value. Keychain doubles are `secrets` types under
//! the same `SecretStore` contract — never the real keychain (`.agents/rules/tests.md`) —
//! and the fixtures are obviously fake.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use secrets::{MemorySecretStore, SecretError, SecretKey, SecretStore, SecretValue};
use storage::{LocalStore, LocalValue};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::env_editor::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

use super::SecretInputs;

/// The workspace every fixture's rows are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture values — obviously fake (`.agents/rules/tests.md`).
const VALUE_FIRST: &str = "token-REDACTED-first";
const VALUE_SECOND: &str = "token-REDACTED-second";
const VALUE_NEW_FIRST: &str = "token-REDACTED-new-first";
const VALUE_NEW_SECOND: &str = "token-REDACTED-new-second";
const LOCAL_OLD: &str = "local-REDACTED-old";

/// A keychain that fails chosen `set` calls and delegates the rest to memory: what a
/// flaky keychain looks like. `fail_sets` holds the 1-based numbers of the `set` calls
/// that fail, so a test can fail the second write and the rollback after it.
struct FailingKeychain {
    inner: MemorySecretStore,
    sets: Mutex<usize>,
    fail_sets: Vec<usize>,
}

impl FailingKeychain {
    fn new(fail_sets: Vec<usize>) -> Self {
        Self {
            inner: MemorySecretStore::new(),
            sets: Mutex::new(0),
            fail_sets,
        }
    }

    /// Seed a value without counting it as one of the save's writes, so `fail_sets`
    /// numbers the writes the save itself makes.
    fn seed(&self, key: &SecretKey, value: &SecretValue) {
        self.inner
            .set(key, value)
            .expect("the fixture secret is seeded");
    }
}

impl SecretStore for FailingKeychain {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        let mut sets = self
            .sets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *sets += 1;
        if self.fail_sets.contains(&*sets) {
            return Err(SecretError::BackendUnavailable {
                reason: "the test keychain refuses this write".to_string(),
            });
        }
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)
    }
}

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

/// A workspace folder holding `pustman.json` and the fixture environment's file.
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
fn a_typed_secret_survives_a_kind_flip_round_trip(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_FIRST);
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            // The user rotates the token — typed, not saved yet — and flips the row's
            // kind round trip before saving.
            assert!(editor.set_secret_value("api_token", VALUE_NEW_FIRST, cx));
            assert!(editor.set_secret("api_token", false, window, cx));
            assert!(editor.set_secret("api_token", true, window, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        Some(VALUE_NEW_FIRST.to_string()),
        "the typed value survives the kind flip; the rotation is not silently lost"
    );
}

#[gpui::test]
fn a_later_keychain_write_failure_restores_the_earlier_one(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("first"), secret_row("second")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    seed_local(&store, &environment, "first", LOCAL_OLD);
    let secrets = Arc::new(FailingKeychain::new(vec![2]));
    secrets.seed(
        &secret_key(&environment, "first"),
        &SecretValue::new(VALUE_FIRST),
    );
    secrets.seed(
        &secret_key(&environment, "second"),
        &SecretValue::new(VALUE_SECOND),
    );
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store.clone(), environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret_value("first", VALUE_NEW_FIRST, cx));
            assert!(editor.set_secret_value("second", VALUE_NEW_SECOND, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save whose keychain write failed keeps the editor open"
    );
    let error = editor_error(&editor, cx);
    assert!(
        error.contains("could not be saved to the OS keychain"),
        "the message says the secret values could not be saved (got: {error})"
    );
    assert!(
        error.contains("restored"),
        "the message says the earlier write was rolled back (got: {error})"
    );
    assert_eq!(
        stored(&*secrets, &environment, "first"),
        Some(VALUE_FIRST.to_string()),
        "the earlier keychain write is rolled back to its previous value"
    );
    assert_eq!(
        stored(&*secrets, &environment, "second"),
        Some(VALUE_SECOND.to_string()),
        "the write that failed never landed"
    );
    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file is still there");
    assert!(
        !String::from_utf8_lossy(&file_bytes).contains("token-REDACTED-new"),
        "the shared file is untouched by the failed save"
    );
    assert_eq!(
        local_rows(&store),
        vec![("first".to_string(), LOCAL_OLD.to_string())],
        "the local values are untouched by the failed save"
    );
}

#[gpui::test]
fn a_failed_restore_names_the_keys_that_may_hold_the_new_value(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("first"), secret_row("second")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    // The second write fails and so does the rollback after it: the first key may now
    // hold the new value and the message must say so — by key, never by value.
    let secrets = Arc::new(FailingKeychain::new(vec![2, 3]));
    secrets.seed(
        &secret_key(&environment, "first"),
        &SecretValue::new(VALUE_FIRST),
    );
    secrets.seed(
        &secret_key(&environment, "second"),
        &SecretValue::new(VALUE_SECOND),
    );
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret_value("first", VALUE_NEW_FIRST, cx));
            assert!(editor.set_secret_value("second", VALUE_NEW_SECOND, cx));
        })
    });
    draw_window(cx);
    save(cx);

    let error = editor_error(&editor, cx);
    assert!(
        error.contains("may now hold the new value for `first`"),
        "the message names the variable key whose restore failed (got: {error})"
    );
    assert!(
        !error.contains(VALUE_NEW_FIRST) && !error.contains(VALUE_NEW_SECOND),
        "the message never carries a value (got: {error})"
    );
}
