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
use std::sync::{Arc, Mutex};

use api_model::{ItemId, Variable, VariableKind};
use environment_picker::env_editor::{self, EditTarget, EnvEditorInputs};
use environment_picker::secret_values::{self, SecretInputs};
use gpui::{Entity, TestAppContext, VisualTestContext};
use secrets::{MemorySecretStore, SecretKey, SecretStore};
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

/// A workspace folder at `root` (PRD §6.1) with the two sources beside it.
fn seed() -> Fixture {
    let temp = TempDir::new().expect("a scratch folder");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("the workspace folder is created");
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Import fixture workspace",
        "format_version": "1.0",
    }))
    .expect("the workspace document serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file writes");
    let environment_source = temp.path().join("staging.postman_environment.json");
    let bytes =
        serde_json::to_vec_pretty(&staging_environment_json()).expect("the fixture serializes");
    std::fs::write(&environment_source, bytes).expect("the environment fixture writes");
    let globals_source = temp.path().join("globals.postman_globals.json");
    let bytes = serde_json::to_vec_pretty(&globals_json()).expect("the fixture serializes");
    std::fs::write(&globals_source, bytes).expect("the globals fixture writes");
    Fixture {
        _temp: temp,
        root,
        environment_source,
        globals_source,
    }
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
    store
}

/// The import of `source`, driven through the door the file dialog's answer lands in —
/// the dialog itself is the platform's and cannot appear in a test.
fn run_import(fixture: &Fixture, source: &Path, cx: &mut VisualTestContext) {
    let root = fixture.root.clone();
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
fn globals_variables(fixture: &Fixture) -> Vec<Variable> {
    let snapshot = workspace_store::open(&fixture.root).expect("the workspace still opens");
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
    run_import(&fixture, &fixture.environment_source, cx);

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
    run_import(&fixture, &fixture.globals_source, cx);

    let globals = globals_variables(&fixture);
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
