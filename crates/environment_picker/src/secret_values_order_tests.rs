//! The order one save lands its keychain writes in, and the destructive paths around
//! them (PRD REQ-VAR-SHARE-02): a key this save sets is never removed by another row's
//! rename or flip in the same save, removals run only after the shared file has landed
//! (so a failure leaves at worst an orphaned entry, never a lost one), an entry the
//! editor could not read is never deleted blind, and no failure path falls back to
//! writing a secret value anywhere else. Keychain doubles are `secrets` types under the
//! same `SecretStore` contract — never the real keychain (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use secrets::{MemorySecretStore, SecretError, SecretKey, SecretStore, SecretValue};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::env_editor::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

use super::SecretInputs;

/// The workspace every fixture's rows are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture secret values — obviously fake (`.agents/rules/tests.md`).
const VALUE_TOKEN: &str = "token-REDACTED-old";
const VALUE_B: &str = "token-REDACTED-b";
const VALUE_X: &str = "token-REDACTED-x";
const VALUE_Y: &str = "token-REDACTED-y";
const VALUE_NEW: &str = "token-REDACTED-new";

/// The shared value the tests write as a marker of "the file has landed".
const FILE_MARKER: &str = "https://marker.example.test";

/// A keychain that records the shared file's bytes at the moment a removal ran: the
/// proof that the file landed before any entry was removed.
struct RecordingStore {
    inner: MemorySecretStore,
    environment_file: PathBuf,
    file_at_removal: Mutex<Option<String>>,
}

impl RecordingStore {
    fn new(environment_file: PathBuf) -> Self {
        Self {
            inner: MemorySecretStore::new(),
            environment_file,
            file_at_removal: Mutex::new(None),
        }
    }

    /// What the shared file held when the first removal ran.
    fn file_at_removal(&self) -> Option<String> {
        self.file_at_removal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl SecretStore for RecordingStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        let mut seen = self
            .file_at_removal
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if seen.is_none() {
            *seen = std::fs::read(&self.environment_file)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        }
        drop(seen);
        self.inner.delete(key)
    }
}

/// A keychain that cannot be read but records every write and removal: what a half-broken
/// keychain looks like. Nothing may be written or removed behind the user's back while
/// the editor cannot see what is stored.
struct UnreadableKeychain {
    inner: MemorySecretStore,
    writes: Mutex<Vec<String>>,
}

impl UnreadableKeychain {
    fn new() -> Self {
        Self {
            inner: MemorySecretStore::new(),
            writes: Mutex::new(Vec::new()),
        }
    }

    /// Every write and removal the store was asked for, in order.
    fn writes(&self) -> Vec<String> {
        self.writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl SecretStore for UnreadableKeychain {
    fn get(&self, _key: &SecretKey) -> Result<SecretValue, SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: "the test keychain cannot be read".to_string(),
        })
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        self.writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(format!("set:{}", key.account()));
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(format!("remove:{}", key.account()));
        self.inner.delete(key)
    }
}

/// One secret fixture row: a secret variable with no value anywhere — the shape
/// REQ-VAR-SHARE-02 writes to the file.
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

/// The local store the editor saves local values into.
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
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

/// A workspace window with this crate wired up to `secret_store` — [`None`] is a run with
/// no keychain connected at all — drawn and active, with the Save keystroke bound over
/// the editor's key context.
fn editor_window(
    cx: &mut TestAppContext,
    secret_store: Option<Arc<dyn SecretStore>>,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        if let Some(store) = secret_store {
            super::install_inputs(cx, SecretInputs { store });
        }
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
fn rotation_moves_the_value_to_the_new_key_and_keeps_it(cx: &mut TestAppContext) {
    // Row order matters here: the row that removes the old `token` key comes *after* the
    // row that sets the new `token`, so an unordered or row-ordered apply drops the value
    // the rotation just moved.
    let environment = staging(vec![secret_row("b"), secret_row("token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "b", VALUE_B);
    seed(&*secrets, &environment, "token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    // The rotation: `b` grows into the `token` name with the new value, the old `token`
    // row is renamed away and its value cleared.
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_key("b", "token", cx));
            assert!(editor.set_secret_value("b", VALUE_NEW, cx));
            assert!(editor.set_key("token", "token_old", cx));
            assert!(editor.set_secret_value("token", "", cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );
    assert_eq!(
        stored(&*secrets, &environment, "token"),
        Some(VALUE_NEW.to_string()),
        "the rotation keeps the new value under its new key"
    );
    assert_eq!(
        stored(&*secrets, &environment, "b"),
        None,
        "the key the value moved away from is cleaned up"
    );
    assert_eq!(
        stored(&*secrets, &environment, "token_old"),
        None,
        "the renamed-away row keeps no entry under its new key"
    );
}

#[gpui::test]
fn swapping_two_secret_keys_keeps_both_values_under_their_new_keys(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("x"), secret_row("y")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "x", VALUE_X);
    seed(&*secrets, &environment, "y", VALUE_Y);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_key("x", "y", cx), "the x row takes the y name");
            assert!(editor.set_key("y", "x", cx), "the y row takes the x name");
        })
    });
    draw_window(cx);
    save(cx);

    assert_eq!(
        stored(&*secrets, &environment, "x"),
        Some(VALUE_Y.to_string()),
        "the x key now holds the value the y row carried"
    );
    assert_eq!(
        stored(&*secrets, &environment, "y"),
        Some(VALUE_X.to_string()),
        "the y key now holds the value the x row carried — no removal ate a value this save set"
    );
}

#[gpui::test]
fn a_flipped_row_removes_its_entry_only_after_the_shared_file_landed(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(RecordingStore::new(
        root.path().join("environments/staging.environment.json"),
    ));
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );
    let at_removal = secrets.file_at_removal();
    let at_removal = at_removal.expect("the entry was removed after the save");
    assert!(
        at_removal.contains("\"default\""),
        "the removal runs only after the file that records the flip landed (file at removal: {at_removal})"
    );
}

#[gpui::test]
fn a_shared_file_write_that_fails_leaves_every_pre_existing_secret_readable(
    cx: &mut TestAppContext,
) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
        })
    });
    // A workspace that declares a newer format version makes every write in it fail after
    // everything else has run — the shared-file write failing, exactly when it must not
    // take a keychain value with it.
    write_future_workspace(root.path());
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save that could not write the file keeps the editor open"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the failure is shown");
    assert!(
        error.contains("shared file is unchanged"),
        "the failure names the part that did not land (got: {error})"
    );
    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        Some(VALUE_TOKEN.to_string()),
        "a failed shared-file write leaves every pre-existing secret value readable"
    );
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

#[gpui::test]
fn a_stale_refusal_leaves_every_pre_existing_secret_readable(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
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

    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the refusal says why");
    assert!(
        error.to_lowercase().contains("changed on disk"),
        "the refusal names what happened (got: {error})"
    );
    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        Some(VALUE_TOKEN.to_string()),
        "a stale refusal leaves every pre-existing secret value readable"
    );
}

#[gpui::test]
fn an_empty_field_removes_the_known_entry_after_the_file_landed(cx: &mut TestAppContext) {
    let environment = staging(vec![
        secret_row("api_token"),
        default_row("base_url", "https://staging.example.test"),
    ]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(RecordingStore::new(
        root.path().join("environments/staging.environment.json"),
    ));
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(
                editor.set_secret_value("api_token", "", cx),
                "the field that showed the stored value is cleared"
            );
            // A marker the saved file carries, so the recording can tell the landed file
            // from the one this editor read.
            assert!(editor.set_shared_value("base_url", FILE_MARKER, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );
    let at_removal = secrets.file_at_removal();
    let at_removal = at_removal.expect("the entry was removed after the save");
    assert!(
        at_removal.contains(FILE_MARKER),
        "the removal runs only after the shared file landed (file at removal: {at_removal})"
    );
}

#[gpui::test]
fn an_entry_the_editor_could_not_read_is_never_deleted_blind(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(UnreadableKeychain::new());
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the keychain failure is shown to the user");
    assert!(
        error.contains("could not be read"),
        "the unreadable keychain is reported, never hidden (got: {error})"
    );

    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "an unchanged save lands and closes the editor"
    );
    assert!(
        secrets.writes().is_empty(),
        "an entry the editor could not read is never deleted or written blind (got: {:?})",
        secrets.writes()
    );
}

#[gpui::test]
fn a_row_flipped_to_default_removes_its_stored_value(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert_eq!(
        stored(&*secrets, &environment, "api_token"),
        None,
        "a row that is no longer a secret takes its stored value with it"
    );
}

#[gpui::test]
fn saving_a_secret_with_no_keychain_writes_nowhere_and_says_so(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx, None);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret_value("api_token", VALUE_NEW, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save that cannot keep the secret is refused and keeps the editor open"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the refusal says why");
    assert!(
        error.contains("Nothing was saved"),
        "the refusal says nothing landed (got: {error})"
    );
    assert!(
        error.contains("not connected"),
        "the refusal names the missing keychain (got: {error})"
    );
    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file is still there");
    assert!(
        !String::from_utf8_lossy(&file_bytes).contains(VALUE_NEW),
        "the refused save writes the value nowhere — not the file"
    );
    let fresh = LocalStore::open(root.path().join("local.db")).expect("the store reopens");
    assert!(
        fresh
            .local_values()
            .list(WORKSPACE_ID)
            .expect("the local values read")
            .is_empty(),
        "the refused save writes the value nowhere — not storage"
    );
}

#[gpui::test]
fn flipping_a_row_to_secret_again_keeps_the_stored_value(cx: &mut TestAppContext) {
    let environment = staging(vec![secret_row("api_token")]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    seed(&*secrets, &environment, "api_token", VALUE_TOKEN);
    let (workspace, cx) = editor_window(cx, Some(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret("api_token", false, window, cx));
            assert!(
                editor.set_secret("api_token", true, window, cx),
                "the row flips back to Secret"
            );
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
        Some(VALUE_TOKEN.to_string()),
        "a flip round trip leaves the stored value alone — an empty field after a flip never deletes it"
    );
}
