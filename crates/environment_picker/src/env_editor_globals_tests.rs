//! The globals path: the workspace's own `pustman.json` carries the globals' shared
//! values in its `variables` field (module docs), written through `workspace_store` like
//! every other item. A `kind: secret` global records no value there — not the one its row
//! shows and not one a hand edit smuggled into the file (REQ-VAR-SHARE-02).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{ItemId, Workspace as WorkspaceModel};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

/// The workspace every fixture's rows and local values are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture secret smuggled into the file — obviously fake (`.agents/rules/tests.md`).
const SMUGGLED_SECRET: &str = "token-REDACTED-global";

/// The global whose shared value the test edits.
const API_URL: &str = "api_url";
const API_URL_FIRST: &str = "https://api.example.test";
const API_URL_EDIT: &str = "https://v2.example.test";

/// The globals as a hand-edited `pustman.json` carries them: raw JSON, so the secret's
/// value survives into the file the way a hand edit would leave it.
fn globals() -> serde_json::Value {
    serde_json::json!([
        {"key": API_URL, "value": API_URL_FIRST, "kind": "default", "enabled": true},
        {"key": "api_token", "value": SMUGGLED_SECRET, "kind": "secret", "enabled": true},
    ])
}

/// A workspace folder whose `pustman.json` carries `globals()`.
fn workspace_with_globals(root: &Path) -> WorkspaceModel {
    let mut extra = BTreeMap::new();
    extra.insert("variables".to_string(), globals());
    let workspace_file = WorkspaceModel {
        id: ItemId::new(),
        name: "Fixture workspace".to_string(),
        format_version: "1.0".to_string(),
        extra,
    };
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_vec_pretty(&workspace_file).expect("the workspace file serializes"),
    )
    .expect("pustman.json is written");
    workspace_file
}

/// The local store the editor saves local values into.
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The inputs one open round runs with: the workspace itself as the globals' scope.
fn inputs(
    root: &Path,
    store: Arc<Mutex<LocalStore>>,
    workspace_file: WorkspaceModel,
) -> EnvEditorInputs {
    let item = Item {
        body: ItemBody::Workspace(workspace_file),
        dir: PathBuf::new(),
        path: Some(PathBuf::from("pustman.json")),
    };
    let target = EditTarget::new(item).expect("the globals are editable");
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
fn globals_save_to_pustman_json_with_no_secret_values(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the workspace folder opens");
    let workspace_file = workspace_with_globals(root.path());
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(cx, &workspace, inputs(root.path(), store, workspace_file));

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(
                editor.set_shared_value(API_URL, API_URL_EDIT, cx),
                "an ordinary global's shared value is editable"
            );
        })
    });
    draw_window(cx);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);

    let bytes =
        std::fs::read(root.path().join("pustman.json")).expect("the workspace file was written");
    let file: serde_json::Value = serde_json::from_slice(&bytes).expect("pustman.json is JSON");
    let variables = file["variables"]
        .as_array()
        .expect("pustman.json carries the globals under `variables`");
    let by_key = |key: &str| {
        variables
            .iter()
            .find(|variable| variable["key"] == key)
            .expect("the global is in the file")
    };
    assert_eq!(
        by_key(API_URL)["value"].as_str(),
        Some(API_URL_EDIT),
        "the edited global's shared value is saved under `variables`"
    );
    assert!(
        by_key("api_token").get("value").is_none(),
        "a secret global records no value"
    );
    assert!(
        !String::from_utf8_lossy(&bytes).contains(SMUGGLED_SECRET),
        "the value a hand edit smuggled into the secret is gone from the file bytes"
    );
}
