//! Importing a Postman environment or globals file (S45b, high-stakes): the untrusted
//! file the user picked becomes one workspace file that declares its secret keys and
//! keeps no values — and every `secret` value goes to the OS keychain under the same
//! installation key the app reads it back with (S43w: `pustman::scope_wiring` composes
//! its reads through `secrets::SecretKey::new(workspace_key, scope_id, variable_key)`,
//! the one constructor both spellings bottom out in).
//!
//! The one test walks both file kinds in one session and pins the promises end to end:
//! a fresh id this build minted (never the file's planted one), the shared value in the
//! file, each secret declared as `Secret` with no value, no workspace file and no toast
//! carrying a value, and each value retrievable from an in-memory secret store under
//! exactly the key the app would read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use api_model::{ItemId, Variable, VariableKind};
use environment_picker::env_editor::{self, EditTarget, EnvEditorInputs};
use environment_picker::secret_values::{self, SecretInputs};
use gpui::{Entity, TestAppContext, VisualTestContext};
use secrets::{MemorySecretStore, SecretError, SecretKey, SecretStore, SecretValue};
use storage::LocalStore;
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

use super::begin_import_file;

/// The fake secret values the fixtures carry — obviously fake, so secret scanners and
/// reviewers never trip on them (`.agents/rules/tests.md`). Distinct per scope, so a
/// value found under the wrong key fails loudly.
const FIXTURE_ENVIRONMENT_SECRET: &str = "token-REDACTED-example";
const FIXTURE_GLOBALS_SECRET: &str = "globals-token-REDACTED-example";

/// The fake installation key the app's wire would install (S43w's `workspace_key`
/// shape): what every write in the import must be keyed by.
const WORKSPACE_KEY: &str = "S45BTESTID0123456789ABCDEF.0123456789abcdef";

/// A real ULID planted in the environment file — if any code ever adopted a file's id,
/// it would adopt this one.
const PLANTED_ENVIRONMENT_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The workspace folder and the two import sources the test runs against. Both sources
/// sit beside the workspace, never inside it.
struct Fixture {
    /// Held so the folders outlive the test.
    _temp: TempDir,
    root: PathBuf,
    environment_source: PathBuf,
    globals_source: PathBuf,
}

/// The Postman environment export the first source holds: one shared variable, one
/// `secret`-typed variable whose value must reach the keychain and nothing else, and a
/// planted id the import must never adopt.
fn staging_environment_json() -> serde_json::Value {
    serde_json::json!({
        "id": PLANTED_ENVIRONMENT_ID,
        "name": "Staging",
        "values": [
            { "key": "base_url", "value": "https://api.test", "type": "default", "enabled": true },
            { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret", "enabled": true }
        ],
        "_postman_variable_scope": "environment",
        "_postman_exported_at": "2026-01-01T00:00:00.000Z"
    })
}

/// The Postman globals export the second source holds: the same shape, one scope up.
fn globals_json() -> serde_json::Value {
    serde_json::json!({
        "name": "globals",
        "values": [
            { "key": "global_token", "value": FIXTURE_GLOBALS_SECRET, "type": "secret" },
            { "key": "trace", "value": "on", "type": "default" }
        ],
        "_postman_variable_scope": "globals"
    })
}

/// A workspace folder with the two standard sources beside it.
fn seed() -> Fixture {
    let (temp, root) = scratch_with(&serde_json::Map::new());
    let environment_source = write_source(
        &root,
        "staging.postman_environment.json",
        &staging_environment_json(),
    );
    let globals_source = write_source(&root, "globals.postman_globals.json", &globals_json());
    Fixture {
        _temp: temp,
        root,
        environment_source,
        globals_source,
    }
}

/// A scratch folder with a workspace folder in it whose `pustman.json` carries
/// `workspace_extras` on top of the plain document.
fn scratch_with(
    workspace_extras: &serde_json::Map<String, serde_json::Value>,
) -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("the workspace folder is created");
    let mut document = serde_json::Map::new();
    document.insert(
        String::from("id"),
        serde_json::Value::from(ItemId::new().to_string()),
    );
    document.insert(
        String::from("name"),
        serde_json::Value::from("Import fixture workspace"),
    );
    document.insert(
        String::from("format_version"),
        serde_json::Value::from("1.0"),
    );
    for (key, value) in workspace_extras {
        document.insert(key.clone(), value.clone());
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(document))
        .expect("the workspace document serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file writes");
    (temp, root)
}

/// Write one import source beside the workspace — never inside it.
fn write_source(root: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let source = root
        .parent()
        .expect("the workspace folder sits in a scratch folder")
        .join(name);
    let bytes = serde_json::to_vec_pretty(value).expect("the fixture serializes");
    std::fs::write(&source, bytes).expect("the fixture writes");
    source
}

/// One window on the app's real init order — `workspace::init` gives it its
/// [`WorkspaceOpen`], `crate::init` registers the panel and this step's import wiring.
fn app_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.run_until_parked();
    (workspace, cx)
}

/// Open `root` as this window's workspace through the one door every open lands in.
fn open_workspace_folder(workspace: &Entity<Workspace>, root: &Path, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| {
            opener.open_folder(root.to_path_buf(), cx);
        });
    });
    cx.run_until_parked();
}

/// Install the keychain the import writes to (an in-memory store, never the OS one) and
/// the installation key the app's wire installs — exactly the two app-globals the import
/// reads, so the test walks the production path. Returns the store to read back from.
fn install_keychain(root: &Path, cx: &mut TestAppContext) -> Arc<dyn SecretStore> {
    let store: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    install_keychain_with(root, cx, Arc::clone(&store));
    store
}

/// [`install_keychain`] with a store of the test's own — an observing or failing fake.
fn install_keychain_with(root: &Path, cx: &mut TestAppContext, store: Arc<dyn SecretStore>) {
    let workspace_item = Item {
        body: ItemBody::Workspace(api_model::Workspace {
            id: ItemId::new(),
            name: String::from("Fixture workspace"),
            format_version: String::from("1.0"),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: Some(PathBuf::from("pustman.json")),
    };
    cx.update(|cx| {
        secret_values::install_inputs(
            cx,
            SecretInputs {
                store: Arc::clone(&store),
            },
        );
        env_editor::install_inputs(
            cx,
            EnvEditorInputs {
                workspace_id: String::from(WORKSPACE_KEY),
                workspace_root: root.to_path_buf(),
                target: EditTarget::new(workspace_item)
                    .expect("a workspace document is the globals scope"),
                local_values: Vec::new(),
                store: Arc::new(Mutex::new(
                    LocalStore::open_in_memory().expect("the in-memory local store opens"),
                )),
            },
        );
    });
}

/// The import of `source` into `root`, driven through the door the file dialog's answer
/// lands in — the dialog itself is the platform's and cannot appear in a test.
fn run_import(root: &Path, source: &Path, cx: &mut VisualTestContext) {
    let root = root.to_path_buf();
    let source = source.to_path_buf();
    cx.update(|window, cx| begin_import_file(root, source, window.window_handle(), cx));
    cx.run_until_parked();
}

/// The item called `name` in `snapshot`, wherever its file landed.
fn item_named<'a>(
    snapshot: &'a workspace_store::WorkspaceSnapshot,
    name: &str,
) -> Option<&'a Item> {
    snapshot.items.values().find(|item| item.name() == name)
}

/// Whether any file under `root` holds `needle` — the high-stakes promise: an imported
/// secret value never lands in a workspace file.
fn any_file_under_contains(root: &Path, needle: &str) -> bool {
    fn walk(dir: &Path, needle: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path, needle) {
                    return true;
                }
            } else if let Ok(bytes) = std::fs::read(&path)
                && String::from_utf8_lossy(&bytes).contains(needle)
            {
                return true;
            }
        }
        false
    }
    walk(root, needle)
}

/// Whether `message` is on screen as a toast (the `toast_layer_tests` shape: the message
/// key must outlive the frame `debug_bounds` reads, so the selector is leaked).
fn toast_shows(cx: &mut VisualTestContext, message: &str) -> bool {
    draw_window(cx);
    let message_selector: &'static str = toast_message_selector(message).leak();
    cx.debug_bounds(message_selector).is_some()
}

/// The globals `pustman.json` carries (env_editor's `variables` field), read back as the
/// variable list the editor reads.
fn globals_variables(root: &Path) -> Vec<Variable> {
    let snapshot = workspace_store::open(root).expect("the workspace still opens");
    let workspace_item = snapshot
        .items
        .values()
        .find(|item| matches!(item.body, ItemBody::Workspace(_)))
        .expect("the workspace document is there");
    let ItemBody::Workspace(document) = &workspace_item.body else {
        panic!("the matched item is the workspace document");
    };
    let value = document
        .extra
        .get("variables")
        .expect("the globals field landed")
        .clone();
    serde_json::from_value(value).expect("the globals field is a variable list")
}

#[gpui::test]
fn import_environment_secrets_go_to_keychain(cx: &mut TestAppContext) {
    let fixture = seed();
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &fixture.root, cx);
    let keychain = install_keychain(&fixture.root, cx);

    // — the environment file —
    run_import(&fixture.root, &fixture.environment_source, cx);

    let snapshot = workspace_store::open(&fixture.root).expect("the workspace still opens");
    let environment = item_named(&snapshot, "Staging").expect("the environment landed");
    let environment_id = environment.id().expect("the environment carries its id");
    assert_eq!(
        snapshot.path(&environment_id),
        Some(Path::new("environments/staging.environment.json")),
        "the environment is written under its own slug"
    );
    let ItemBody::Environment(imported) = &environment.body else {
        panic!("the landed item is the imported environment");
    };
    assert_ne!(
        imported.id.to_string(),
        PLANTED_ENVIRONMENT_ID,
        "under an id this build minted, never the file's planted one"
    );
    let base_url = imported
        .variables
        .iter()
        .find(|variable| variable.key == "base_url")
        .expect("the shared variable was imported");
    assert_eq!(
        base_url.value.as_deref(),
        Some("https://api.test"),
        "the shared value lands in the file"
    );
    let api_key = imported
        .variables
        .iter()
        .find(|variable| variable.key == "api_key")
        .expect("the secret is declared in the file");
    assert_eq!(api_key.kind, VariableKind::Secret, "as a secret");
    assert_eq!(api_key.value, None, "keeping no value in the model");
    assert!(
        !any_file_under_contains(&fixture.root, FIXTURE_ENVIRONMENT_SECRET),
        "and no workspace file carries the imported secret value"
    );

    // The value is under the key the app reads it back with: the workspace key the wire
    // installed, the environment's own scope id and the variable key (S43w).
    let key = SecretKey::new(WORKSPACE_KEY, imported.id.to_string(), "api_key")
        .expect("the key is representable");
    let stored = keychain
        .get(&key)
        .expect("the secret value is in the keychain");
    assert_eq!(
        stored.expose(),
        FIXTURE_ENVIRONMENT_SECRET,
        "the keychain holds the value under the key the app reads"
    );
    let success = "Imported “Staging” — 2 variables, 1 secret value stored in the keychain.";
    assert!(toast_shows(cx, success), "the import reports itself");
    assert!(
        !success.contains(FIXTURE_ENVIRONMENT_SECRET),
        "and the report never echoes a secret value"
    );

    // — the globals file —
    run_import(&fixture.root, &fixture.globals_source, cx);

    let globals = globals_variables(&fixture.root);
    let trace = globals
        .iter()
        .find(|variable| variable.key == "trace")
        .expect("the shared global was imported");
    assert_eq!(
        trace.value.as_deref(),
        Some("on"),
        "into the workspace's globals"
    );
    let global_token = globals
        .iter()
        .find(|variable| variable.key == "global_token")
        .expect("the secret global is declared in the file");
    assert_eq!(global_token.kind, VariableKind::Secret, "as a secret");
    assert_eq!(global_token.value, None, "keeping no value in the model");
    assert!(
        !any_file_under_contains(&fixture.root, FIXTURE_GLOBALS_SECRET),
        "and no workspace file carries the imported global secret value"
    );

    let key =
        SecretKey::new(WORKSPACE_KEY, "globals", "global_token").expect("the key is representable");
    let stored = keychain
        .get(&key)
        .expect("the global secret is in the keychain");
    assert_eq!(
        stored.expose(),
        FIXTURE_GLOBALS_SECRET,
        "under the globals scope id the editor keys the globals by"
    );
    assert!(
        toast_shows(
            cx,
            "Imported globals — 2 variables, 1 secret value stored in the keychain."
        ),
        "the globals import reports itself"
    );
}

// ------------------------------------------------------------------------------------
// Fix cycle 1: a duplicated key is merged (and its value never reaches a file), the
// keychain lands before the file, globals replacements are named with no orphaned
// secret left behind, and a partial keychain failure rolls everything back.

/// A second and third obviously fake secret value (`.agents/rules/tests.md`), distinct
/// per key so a value found under the wrong key fails loudly.
const FIXTURE_SECOND_SECRET: &str = "second-token-REDACTED-example";
const FIXTURE_THIRD_SECRET: &str = "third-token-REDACTED-example";

/// A [`SecretStore`] over an in-memory store that records whether the import's file
/// already existed when a value was stored — fix 2's ordering proof.
struct ObservingStore {
    inner: MemorySecretStore,
    environments_dir: PathBuf,
    saw_file: AtomicBool,
}

impl ObservingStore {
    fn new(environments_dir: PathBuf) -> Self {
        Self {
            inner: MemorySecretStore::new(),
            environments_dir,
            saw_file: AtomicBool::new(false),
        }
    }

    fn saw_file(&self) -> bool {
        self.saw_file.load(Ordering::SeqCst)
    }
}

impl SecretStore for ObservingStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        if self.environments_dir.exists() {
            self.saw_file.store(true, Ordering::SeqCst);
        }
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)
    }
}

/// A [`SecretStore`] over an in-memory store that refuses the n-th `set` and every
/// `delete` of one chosen key — fix 4's partial-failure proof.
struct FailingStore {
    inner: MemorySecretStore,
    sets: AtomicUsize,
    /// The `set` call number this store starts refusing at (one-based).
    fail_set_at: usize,
    /// The variable key whose `delete` always fails: what cannot be rolled back.
    fail_delete_for: Option<String>,
    /// Every key this store was asked about — how the test finds the keys the import
    /// minted a scope id for (the file never lands when the writes fail).
    seen: Mutex<Vec<SecretKey>>,
}

impl FailingStore {
    fn new(fail_set_at: usize, fail_delete_for: &str) -> Self {
        Self {
            inner: MemorySecretStore::new(),
            sets: AtomicUsize::new(0),
            fail_set_at,
            fail_delete_for: Some(String::from(fail_delete_for)),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// The recorded key for `name`, whatever scope the import minted for it.
    fn key_for(&self, name: &str) -> SecretKey {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|key| key.variable_key() == name)
            .cloned()
            .unwrap_or_else(|| panic!("the import asked the store about {name}"))
    }
}

impl SecretStore for FailingStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(key.clone());
        let call = self.sets.fetch_add(1, Ordering::SeqCst) + 1;
        if call >= self.fail_set_at {
            return Err(SecretError::BackendUnavailable {
                reason: format!("the test store refused to write `{}`", key.variable_key()),
            });
        }
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(key.clone());
        if self.fail_delete_for.as_deref() == Some(key.variable_key()) {
            return Err(SecretError::BackendUnavailable {
                reason: format!("the test store refused to remove `{}`", key.variable_key()),
            });
        }
        self.inner.delete(key)
    }
}

#[gpui::test]
fn import_environment_dedupe_never_writes_a_duplicated_secret_value(cx: &mut TestAppContext) {
    // One key twice: once plain with the value, once `secret` with the same value — the
    // shape that used to leave a "secret" value on disk next to its Secret row — and a
    // plain key twice. The name is hostile so the same run pins the toast's sanitising.
    let hostile_name = format!("Dedupe\u{7}{}", "x".repeat(100));
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    let source = write_source(
        &root,
        "dedupe.postman_environment.json",
        &serde_json::json!({
            "name": hostile_name,
            "values": [
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "default", "enabled": true },
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret", "enabled": false },
                { "key": "trace", "value": "first", "type": "default" },
                { "key": "trace", "value": "second", "type": "default" }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let keychain = install_keychain(&root, cx);

    run_import(&root, &source, cx);

    let snapshot = workspace_store::open(&root).expect("the workspace still opens");
    let environment = item_named(&snapshot, &hostile_name).expect("the environment landed");
    let ItemBody::Environment(imported) = &environment.body else {
        panic!("the landed item is the imported environment");
    };
    assert_eq!(
        imported
            .variables
            .iter()
            .filter(|variable| variable.key == "api_key")
            .count(),
        1,
        "the duplicated key is merged into one row"
    );
    let api_key = imported
        .variables
        .iter()
        .find(|variable| variable.key == "api_key")
        .expect("the merged row is there");
    assert_eq!(
        api_key.kind,
        VariableKind::Secret,
        "the secret reading wins"
    );
    assert_eq!(
        api_key.value, None,
        "and no occurrence's value reaches the model"
    );
    // The file's `enabled` flag is carried as far as the model allows — the workspace
    // format records only `{key, kind, description}` for a secret, so a dropped flag is
    // reported rather than silently changed (fix 5): the note names the key below.
    let trace = imported
        .variables
        .iter()
        .find(|variable| variable.key == "trace")
        .expect("the plain duplicate is there too");
    assert_eq!(
        trace.value.as_deref(),
        Some("first"),
        "a plain duplicate keeps its first row"
    );
    assert!(
        !any_file_under_contains(&root, FIXTURE_ENVIRONMENT_SECRET),
        "and the duplicated secret value is in no file"
    );

    let key = SecretKey::new(WORKSPACE_KEY, imported.id.to_string(), "api_key")
        .expect("the key is representable");
    let stored = keychain.get(&key).expect("the value is in the keychain");
    assert_eq!(stored.expose(), FIXTURE_ENVIRONMENT_SECRET);

    // The toast names the dedupe (keys only) and shows the sanitised, truncated name:
    // the control character becomes a space and 107 characters end at 80 plus an ellipsis.
    let label = format!("Dedupe {}…", "x".repeat(73));
    let expected = format!(
        "Imported “{label}” — 2 variables, 1 secret value stored in the keychain. — notes: \
         api_key — appears more than once and was merged as one secret with no value written \
         to the file; its enabled flag is dropped because the workspace format keeps no \
         enabled flag for a secret; trace — appears more than once and the rows were merged"
    );
    assert!(
        toast_shows(cx, &expected),
        "the report names the merged keys and never a value"
    );
    assert!(!expected.contains(FIXTURE_ENVIRONMENT_SECRET));
}

#[gpui::test]
fn import_environment_stores_the_keychain_entries_before_the_file_exists(cx: &mut TestAppContext) {
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    let source = write_source(
        &root,
        "staging.postman_environment.json",
        &serde_json::json!({
            "name": "Staging",
            "values": [
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret" },
                { "key": "token_2", "value": FIXTURE_SECOND_SECRET, "type": "secret" }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let observing = Arc::new(ObservingStore::new(root.join("environments")));
    install_keychain_with(&root, cx, observing.clone());

    run_import(&root, &source, cx);

    assert!(
        !observing.saw_file(),
        "every keychain write happened before the workspace file existed"
    );
    let snapshot = workspace_store::open(&root).expect("the workspace still opens");
    let environment = item_named(&snapshot, "Staging").expect("and the file landed afterwards");
    let ItemBody::Environment(imported) = &environment.body else {
        panic!("the landed item is the imported environment");
    };
    for (key_name, value) in [
        ("api_key", FIXTURE_ENVIRONMENT_SECRET),
        ("token_2", FIXTURE_SECOND_SECRET),
    ] {
        let key = SecretKey::new(WORKSPACE_KEY, imported.id.to_string(), key_name)
            .expect("the key is representable");
        let stored = observing.get(&key).expect("the value is in the keychain");
        assert_eq!(stored.expose(), value);
    }
}

#[gpui::test]
fn import_globals_names_every_replaced_key_and_leaves_no_orphaned_secret(cx: &mut TestAppContext) {
    // Seeded globals: one untouched row, one plain row the import replaces, one secret
    // row the import replaces with a new secret, and one secret row the import downgrades
    // to plain — the downgrade that used to orphan its keychain entry.
    let mut extras = serde_json::Map::new();
    extras.insert(
        String::from("variables"),
        serde_json::json!([
            { "key": "keep_me", "value": "keep-value" },
            { "key": "plain", "value": "old-plain-value" },
            { "key": "secret_token", "kind": "secret" },
            { "key": "rotated", "kind": "secret" }
        ]),
    );
    let (_temp, root) = scratch_with(&extras);
    let source = write_source(
        &root,
        "globals.postman_globals.json",
        &serde_json::json!({
            "name": "globals",
            "values": [
                { "key": "plain", "value": "new-plain-value", "type": "default" },
                { "key": "secret_token", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "default" },
                { "key": "rotated", "value": FIXTURE_SECOND_SECRET, "type": "secret" },
                { "key": "new_one", "value": FIXTURE_THIRD_SECRET, "type": "secret" }
            ],
            "_postman_variable_scope": "globals"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let keychain = install_keychain(&root, cx);
    for (key_name, value) in [
        ("secret_token", "old-secret-value"),
        ("rotated", "old-rotated-value"),
    ] {
        let key =
            SecretKey::new(WORKSPACE_KEY, "globals", key_name).expect("the key is representable");
        keychain
            .set(&key, &SecretValue::new(value))
            .expect("the seeded entry stores");
    }

    run_import(&root, &source, cx);

    let globals = globals_variables(&root);
    let row = |name: &str| {
        globals
            .iter()
            .find(|variable| variable.key == name)
            .unwrap_or_else(|| panic!("the {name} row is there"))
            .clone()
    };
    assert_eq!(
        row("keep_me").value.as_deref(),
        Some("keep-value"),
        "an untouched row survives the merge"
    );
    assert_eq!(
        row("plain").value.as_deref(),
        Some("new-plain-value"),
        "a replaced plain row takes the imported value"
    );
    assert_eq!(
        row("secret_token").kind,
        VariableKind::Default,
        "the downgraded row is plain now"
    );
    assert_eq!(
        row("rotated").value,
        None,
        "the replaced secret row keeps no value in the file"
    );
    assert_eq!(row("new_one").value, None, "nor does the new one");
    assert!(
        !any_file_under_contains(&root, "old-secret-value")
            && !any_file_under_contains(&root, "old-rotated-value"),
        "no file ever carried a keychain value"
    );

    let rotated =
        SecretKey::new(WORKSPACE_KEY, "globals", "rotated").expect("the key is representable");
    assert_eq!(
        keychain
            .get(&rotated)
            .expect("the replaced secret is stored")
            .expose(),
        FIXTURE_SECOND_SECRET,
        "the imported secret replaced the old entry"
    );
    let new_one =
        SecretKey::new(WORKSPACE_KEY, "globals", "new_one").expect("the key is representable");
    assert_eq!(
        keychain
            .get(&new_one)
            .expect("the new secret is stored")
            .expose(),
        FIXTURE_THIRD_SECRET
    );
    let downgraded =
        SecretKey::new(WORKSPACE_KEY, "globals", "secret_token").expect("the key is representable");
    assert!(
        keychain.get(&downgraded).is_err(),
        "and the downgraded key left no orphaned keychain entry"
    );

    let expected = "Imported globals — 4 variables, 2 secret values stored in the keychain. — \
                    replaced: plain, secret_token, rotated — no longer a secret: secret_token \
                    (the old keychain entry was removed)";
    assert!(
        toast_shows(cx, expected),
        "every replaced key is named and the downgrade is reported"
    );
    assert!(
        !expected.contains("old-secret-value") && !expected.contains(FIXTURE_SECOND_SECRET),
        "and the message names keys, never values"
    );
}

#[gpui::test]
fn import_environment_partial_keychain_failure_rolls_back_and_names_what_it_could_not(
    cx: &mut TestAppContext,
) {
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    let source = write_source(
        &root,
        "secrets.postman_environment.json",
        &serde_json::json!({
            "name": "Secrets",
            "values": [
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret" },
                { "key": "token_2", "value": FIXTURE_SECOND_SECRET, "type": "secret" },
                { "key": "token_3", "value": FIXTURE_THIRD_SECRET, "type": "secret" }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    // The third write fails, and the rollback cannot take the first entry back out.
    let failing = Arc::new(FailingStore::new(3, "api_key"));
    install_keychain_with(&root, cx, failing.clone());

    run_import(&root, &source, cx);

    let snapshot = workspace_store::open(&root).expect("the workspace still opens");
    assert!(
        item_named(&snapshot, "Secrets").is_none(),
        "the file is not written when the keychain writes cannot all land"
    );
    assert!(
        !root.join("environments").exists(),
        "and no half-written directory is left behind"
    );
    let rolled_back = failing.key_for("token_2");
    assert!(
        failing.get(&rolled_back).is_err(),
        "the entry that was rolled back is gone"
    );
    let never_stored = failing.key_for("token_3");
    assert!(
        failing.get(&never_stored).is_err(),
        "the entry whose write failed was never stored"
    );
    let unrestoreable = failing.key_for("api_key");
    assert!(
        failing.get(&unrestoreable).is_ok(),
        "the entry whose rollback failed is still there — and named in the message"
    );

    let expected = "Could not import secrets.postman_environment.json: a secret value could not \
                    be stored — the OS keychain is unavailable: the test store refused to write \
                    `token_3`. — and the keychain entries under api_key could not be rolled back";
    assert!(
        toast_shows(cx, expected),
        "the message names the failed key and what could not be restored"
    );
    assert!(
        !expected.contains(FIXTURE_ENVIRONMENT_SECRET)
            && !expected.contains(FIXTURE_SECOND_SECRET)
            && !expected.contains(FIXTURE_THIRD_SECRET),
        "and never a value"
    );
}

// ------------------------------------------------------------------------------------
// Fix cycle 2: secret-ness is what the file *declares* (a valueless or empty-valued
// secret never leaves a plain value on disk), a globals save made while the keychain
// writes are in flight is never clobbered, and the message caps its lists and reports
// differing secret values by key only.

/// A [`SecretStore`] over an in-memory store whose first `set` lands a concurrent
/// globals save — fix 2's lost-update proof: the keychain phase is exactly when the
/// environment editor (or a `git pull`) can write `pustman.json` under the import.
struct ConcurrentStore {
    inner: MemorySecretStore,
    root: PathBuf,
    fired: AtomicBool,
}

impl ConcurrentStore {
    fn new(root: PathBuf) -> Self {
        Self {
            inner: MemorySecretStore::new(),
            root,
            fired: AtomicBool::new(false),
        }
    }
}

impl SecretStore for ConcurrentStore {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        if !self.fired.swap(true, Ordering::SeqCst) {
            concurrent_globals_save(&self.root);
        }
        self.inner.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)
    }
}

/// What a concurrent editor session writes: a whole new globals list in `pustman.json`.
fn concurrent_globals_save(root: &Path) {
    let path = root.join("pustman.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("the workspace file reads"))
            .expect("the workspace file parses");
    document["variables"] = serde_json::json!([{ "key": "concurrent_row", "value": "kept" }]);
    let bytes = serde_json::to_vec_pretty(&document).expect("the workspace document serializes");
    std::fs::write(&path, bytes).expect("the concurrent save writes");
}

#[gpui::test]
fn import_environment_declared_secret_never_writes_a_plain_value(cx: &mut TestAppContext) {
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    // Fix 1's two shapes: a declared secret with no value at all, and one with an empty
    // value. Both used to leave the plain twin's value on disk next to the Secret row.
    let valueless = write_source(
        &root,
        "valueless.postman_environment.json",
        &serde_json::json!({
            "name": "Valueless",
            "values": [
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "default" },
                { "key": "api_key", "type": "secret" }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let empty = write_source(
        &root,
        "empty.postman_environment.json",
        &serde_json::json!({
            "name": "Empty",
            "values": [
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "default" },
                { "key": "api_key", "value": "", "type": "secret" }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let keychain = install_keychain(&root, cx);

    run_import(&root, &valueless, cx);
    run_import(&root, &empty, cx);

    for name in ["Valueless", "Empty"] {
        let snapshot = workspace_store::open(&root).expect("the workspace still opens");
        let environment = item_named(&snapshot, name).expect("the environment landed");
        let ItemBody::Environment(imported) = &environment.body else {
            panic!("the landed item is the imported environment");
        };
        let api_key = imported
            .variables
            .iter()
            .find(|variable| variable.key == "api_key")
            .expect("the key is declared in the file");
        assert_eq!(
            api_key.kind,
            VariableKind::Secret,
            "what the file declares wins, value or no value"
        );
        assert_eq!(
            api_key.value, None,
            "and no plain value follows the declared secret into the file"
        );
        let key = SecretKey::new(WORKSPACE_KEY, imported.id.to_string(), "api_key")
            .expect("the key is representable");
        assert!(
            keychain.get(&key).is_err(),
            "and nothing is stored for a secret with no value"
        );
    }
    assert!(
        !any_file_under_contains(&root, FIXTURE_ENVIRONMENT_SECRET),
        "the plain twin's value is in no file"
    );
}

#[gpui::test]
fn import_globals_never_clobbers_a_concurrent_globals_save(cx: &mut TestAppContext) {
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    let source = write_source(
        &root,
        "concurrent.postman_globals.json",
        &serde_json::json!({
            "name": "globals",
            "values": [
                { "key": "imported_row", "value": "imported-value", "type": "default" },
                { "key": "imported_secret", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret" }
            ],
            "_postman_variable_scope": "globals"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let concurrent = Arc::new(ConcurrentStore::new(root.clone()));
    install_keychain_with(&root, cx, concurrent.clone());

    run_import(&root, &source, cx);

    let globals = globals_variables(&root);
    assert_eq!(
        globals.len(),
        1,
        "the import wrote nothing over the concurrent save"
    );
    assert_eq!(
        globals[0].key, "concurrent_row",
        "the concurrent change survives"
    );
    let key = SecretKey::new(WORKSPACE_KEY, "globals", "imported_secret")
        .expect("the key is representable");
    assert!(
        concurrent.get(&key).is_err(),
        "and the keychain write was rolled back"
    );
    let expected = "Could not import the globals: the globals changed while this import was \
                    running — the import is refused; try again. — and the keychain entries this \
                    import wrote were rolled back";
    assert!(
        toast_shows(cx, expected),
        "the import refuses with a clear message instead of clobbering"
    );
}

#[gpui::test]
fn import_globals_caps_replacements_and_reports_differing_secret_values(cx: &mut TestAppContext) {
    let mut extras = serde_json::Map::new();
    extras.insert(
        String::from("variables"),
        serde_json::json!([
            { "key": "r1", "value": "old-1" },
            { "key": "r2", "value": "old-2" },
            { "key": "r3", "value": "old-3" },
            { "key": "r4", "value": "old-4" },
            { "key": "r5", "value": "old-5" }
        ]),
    );
    let (_temp, root) = scratch_with(&extras);
    let source = write_source(
        &root,
        "wide.postman_globals.json",
        &serde_json::json!({
            "name": "globals",
            "values": [
                { "key": "r1", "value": "new-1", "type": "default" },
                { "key": "r2", "value": "new-2", "type": "default" },
                { "key": "r3", "value": "new-3", "type": "default" },
                { "key": "r4", "value": "new-4", "type": "default" },
                { "key": "r5", "value": "new-5", "type": "default" },
                { "key": "api_key", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "secret" },
                { "key": "api_key", "value": FIXTURE_SECOND_SECRET, "type": "secret" }
            ],
            "_postman_variable_scope": "globals"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let keychain = install_keychain(&root, cx);

    run_import(&root, &source, cx);

    let key =
        SecretKey::new(WORKSPACE_KEY, "globals", "api_key").expect("the key is representable");
    assert_eq!(
        keychain
            .get(&key)
            .expect("a secret value is stored")
            .expose(),
        FIXTURE_ENVIRONMENT_SECRET,
        "the first of the differing secret values is the one kept"
    );
    assert!(
        !any_file_under_contains(&root, FIXTURE_ENVIRONMENT_SECRET)
            && !any_file_under_contains(&root, FIXTURE_SECOND_SECRET),
        "neither secret value is in any file"
    );
    let expected = "Imported globals — 6 variables, 1 secret value stored in the keychain. — \
                    replaced: r1, r2, r3 (and 2 more) — notes: api_key — appears more than once \
                    and was merged as one secret with no value written to the file; the duplicate \
                    secret occurrences carry different values and the first one was kept";
    assert!(
        toast_shows(cx, expected),
        "the replacement list is capped and the differing values are reported by key"
    );
    assert!(
        !expected.contains(FIXTURE_ENVIRONMENT_SECRET) && !expected.contains(FIXTURE_SECOND_SECRET),
        "and never a value"
    );
}

// ------------------------------------------------------------------------------------
// Fix cycle 3: keys are read exactly the way `api_model`'s importer reads them (a
// numeric or boolean key is a key), the refusal message claims nothing about what the
// rollback note speaks for, and no message prints a key's control characters.

#[gpui::test]
fn import_environment_numeric_key_secrets_are_declared_and_keep_values_off_disk(
    cx: &mut TestAppContext,
) {
    let (_temp, root) = scratch_with(&serde_json::Map::new());
    // The fix-1 shape exactly: one occurrence spells the key as a string, the other as a
    // number — `split_secrets` reads both as "1", and the declared secret must win.
    let twin = write_source(
        &root,
        "numeric-twin.postman_environment.json",
        &serde_json::json!({
            "name": "Numeric twin",
            "values": [
                { "key": "1", "value": FIXTURE_ENVIRONMENT_SECRET, "type": "default" },
                { "key": 1, "type": "secret", "value": FIXTURE_SECOND_SECRET }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    // And a numeric-key secret on its own: it must declare itself, keychain entry and all.
    let alone = write_source(
        &root,
        "numeric-alone.postman_environment.json",
        &serde_json::json!({
            "name": "Numeric alone",
            "values": [
                { "key": 7, "type": "secret", "value": FIXTURE_THIRD_SECRET }
            ],
            "_postman_variable_scope": "environment"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    let keychain = install_keychain(&root, cx);

    run_import(&root, &twin, cx);
    run_import(&root, &alone, cx);

    let snapshot = workspace_store::open(&root).expect("the workspace still opens");
    for (name, row_key, stored) in [
        ("Numeric twin", "1", FIXTURE_SECOND_SECRET),
        ("Numeric alone", "7", FIXTURE_THIRD_SECRET),
    ] {
        let environment = item_named(&snapshot, name).expect("the environment landed");
        let ItemBody::Environment(imported) = &environment.body else {
            panic!("the landed item is the imported environment");
        };
        let secret_row = imported
            .variables
            .iter()
            .find(|variable| variable.key == row_key)
            .expect("the numeric key is declared as a Secret row");
        assert_eq!(
            secret_row.kind,
            VariableKind::Secret,
            "what the file declares wins, spelled how the importer spells it"
        );
        assert_eq!(secret_row.value, None, "and the row carries no value");
        let key = SecretKey::new(WORKSPACE_KEY, imported.id.to_string(), &secret_row.key)
            .expect("the key is representable");
        assert_eq!(
            keychain
                .get(&key)
                .expect("the entry resolves under the row's key")
                .expose(),
            stored,
            "the keychain entry resolves under that row's key"
        );
    }
    assert!(
        !any_file_under_contains(&root, FIXTURE_ENVIRONMENT_SECRET),
        "the plain twin's value is in no file"
    );
    assert!(
        !any_file_under_contains(&root, FIXTURE_SECOND_SECRET)
            && !any_file_under_contains(&root, FIXTURE_THIRD_SECRET),
        "and no secret value is either"
    );
}

#[gpui::test]
fn import_globals_message_never_prints_a_control_character(cx: &mut TestAppContext) {
    // A replaced key carrying a control character: the message shows the sanitised name
    // — the raw key would push its bytes into the toast line.
    let mut extras = serde_json::Map::new();
    extras.insert(
        String::from("variables"),
        serde_json::json!([{ "key": "r\u{7}1", "value": "old-value" }]),
    );
    let (_temp, root) = scratch_with(&extras);
    let source = write_source(
        &root,
        "bell.postman_globals.json",
        &serde_json::json!({
            "name": "globals",
            "values": [
                { "key": "r\u{7}1", "value": "new-value", "type": "default" }
            ],
            "_postman_variable_scope": "globals"
        }),
    );
    let (workspace, cx) = app_window(cx);
    open_workspace_folder(&workspace, &root, cx);
    install_keychain(&root, cx);

    run_import(&root, &source, cx);

    let expected = "Imported globals — 1 variables, no secret values to store. — replaced: r 1";
    assert!(
        toast_shows(cx, expected),
        "the message names the key with its control characters sanitised away"
    );
}
