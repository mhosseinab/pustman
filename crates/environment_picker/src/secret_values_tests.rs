//! A secret variable's value is set and read through the OS keychain and never written to
//! the environment file or to `storage` (PRD REQ-VAR-SHARE-02). The editor is driven the
//! way a user drives it — `env_editor::Open`, a value typed into the row's masked field,
//! Save as its keystroke — against a real workspace folder on disk and a real
//! `storage::LocalStore`, with a `secrets::MemorySecretStore` standing in for the OS
//! keychain under the same `SecretStore` contract. An editor that let a secret reach the
//! file, SQLite or the display fails these tests, and so does one that hides a keychain
//! failure or falls back to writing the value somewhere else.

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

use crate::env_editor::{
    DisplayedRow, EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs,
};

use super::SecretInputs;

/// The workspace every fixture's rows are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture secret — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";

/// The fixture secret variable's key.
const API_TOKEN: &str = "api_token";

/// A keychain that refuses every operation: what a machine without a usable keychain
/// looks like to the editor. The editor must show the failure and write the value
/// nowhere else — never a fallback store, never the file.
struct UnavailableStore;

impl SecretStore for UnavailableStore {
    fn get(&self, _key: &SecretKey) -> Result<SecretValue, SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: "the test keychain refuses to open".to_string(),
        })
    }

    fn set(&self, _key: &SecretKey, _value: &SecretValue) -> Result<(), SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: "the test keychain refuses to open".to_string(),
        })
    }

    fn delete(&self, _key: &SecretKey) -> Result<(), SecretError> {
        Err(SecretError::BackendUnavailable {
            reason: "the test keychain refuses to open".to_string(),
        })
    }
}

/// The one fixture row: a secret variable with no value anywhere — the shape
/// REQ-VAR-SHARE-02 writes to the file.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![Variable {
            key: API_TOKEN.to_string(),
            value: None,
            kind: VariableKind::Secret,
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

/// A workspace folder holding `pustman.json` and the fixture environment's file, so a
/// save rewrites a real file rather than creating one.
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

/// Every displayed row of `editor`, as the user sees it.
fn displayed_rows(editor: &Entity<EnvEditor>, cx: &VisualTestContext) -> Vec<DisplayedRow> {
    cx.read(|cx| editor.read_with(cx, |editor, cx| editor.displayed_rows(cx)))
}

/// The row for `variable_key`.
fn row_for<'a>(rows: &'a [DisplayedRow], variable_key: &str) -> &'a DisplayedRow {
    rows.iter()
        .find(|row| row.key == variable_key)
        .expect("the variable is listed")
}

/// Save as the user saves it: the Save keystroke over the editor's key context.
fn save(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);
}

/// The keychain key the fixture secret lives under.
fn secret_key(environment: &Environment) -> SecretKey {
    SecretKey::new(WORKSPACE_ID, environment.id.to_string(), API_TOKEN)
        .expect("the fixture key is representable in the keychain")
}

#[gpui::test]
fn secret_value_goes_to_keychain_not_file(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment.clone()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(
                editor.set_secret_value(API_TOKEN, FAKE_SECRET, cx),
                "a secret row's value is typed into its masked field"
            );
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );
    let key = secret_key(&environment);
    let stored = secrets
        .get(&key)
        .expect("the value was set through SecretStore");
    assert_eq!(
        stored.expose(),
        FAKE_SECRET,
        "the secret's value is kept in the keychain"
    );

    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file was written");
    let file: serde_json::Value =
        serde_json::from_slice(&file_bytes).expect("the environment file is JSON");
    assert!(
        file["variables"][0].get("value").is_none(),
        "a secret records no value in the file (REQ-VAR-SHARE-02)"
    );
    assert!(
        !String::from_utf8_lossy(&file_bytes).contains(FAKE_SECRET),
        "the secret's value never reaches the file"
    );

    let fresh = LocalStore::open(root.path().join("local.db")).expect("the store reopens");
    assert!(
        fresh
            .local_values()
            .list(WORKSPACE_ID)
            .expect("the local values read")
            .is_empty(),
        "the secret's value never reaches storage"
    );
}

#[gpui::test]
fn secret_value_is_read_back_into_the_masked_field(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .set(&secret_key(&environment), &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret is seeded");
    let (workspace, cx) = editor_window(cx, Arc::clone(&secrets) as Arc<dyn SecretStore>);
    let editor = open_editor(cx, &workspace, inputs(root.path(), store, environment));

    let field =
        cx.read(|cx| editor.read_with(cx, |editor, _cx| editor.secret_value_field(API_TOKEN)));
    let field = field.expect("a secret row has its masked value field");
    let typed = cx.read(|cx| field.read(cx).text());
    assert_eq!(
        typed, FAKE_SECRET,
        "the value is read back from the keychain through SecretStore"
    );

    let rows = displayed_rows(&editor, cx);
    let shown = row_for(&rows, API_TOKEN)
        .secret
        .as_deref()
        .expect("the secret row shows its masked field");
    assert!(
        !shown.contains(FAKE_SECRET),
        "the display shows masks, never the value"
    );
    assert_eq!(
        shown.chars().count(),
        FAKE_SECRET.chars().count(),
        "one mask stands per character of the value"
    );
}

#[gpui::test]
fn keychain_failure_is_shown_and_the_value_is_written_nowhere_else(cx: &mut TestAppContext) {
    let environment = staging();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx, Arc::new(UnavailableStore));
    let editor = open_editor(cx, &workspace, inputs(root.path(), store, environment));

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.set_secret_value(API_TOKEN, FAKE_SECRET, cx));
        })
    });
    draw_window(cx);
    save(cx);

    assert!(
        editor_of(&workspace, cx).is_some(),
        "a save that could not reach the keychain keeps the editor open"
    );
    let error = cx.read(|cx| editor.read_with(cx, |editor, _| editor.error()));
    let error = error.expect("the keychain failure is shown to the user");
    assert!(
        error.contains("could not be saved to the OS keychain"),
        "the failure names what did not happen and where (got: {error})"
    );
    assert!(
        !error.contains(FAKE_SECRET),
        "an error message never carries a secret value"
    );

    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file is still there");
    assert!(
        !String::from_utf8_lossy(&file_bytes).contains(FAKE_SECRET),
        "a keychain failure does not fall back to writing the value into the file"
    );
    let fresh = LocalStore::open(root.path().join("local.db")).expect("the store reopens");
    assert!(
        fresh
            .local_values()
            .list(WORKSPACE_ID)
            .expect("the local values read")
            .is_empty(),
        "a keychain failure does not fall back to writing the value into storage"
    );
}
