//! The variables editor is driven the way a user drives it: `env_editor::Open` through
//! GPUI's action dispatch, edits typed into the row's own fields, and Save as the
//! keystroke its action is bound to — against a real workspace folder on disk and a real
//! `storage::LocalStore`, both read back the way a reopened app would find them. An
//! editor that only changed memory fails these tests, and so does one that lets a
//! secret's value reach a field or the file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{Entity, KeyBinding, TestAppContext, VisualTestContext};
use storage::{LocalStore, LocalValue};
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, Open, Save, install_inputs};

/// The workspace every fixture's rows and local values are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture secret — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";

/// One variable of the fixture environment.
fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: Some("the API's base URL".to_string()),
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

/// The workspace item the environment's shared values are saved to — the shape
/// `workspace_store::open` hands the app after a workspace opens.
fn environment_item(environment: &Environment) -> Item {
    Item {
        body: ItemBody::Environment(environment.clone()),
        dir: PathBuf::from("environments"),
        path: Some(PathBuf::from("environments/staging.environment.json")),
    }
}

/// A workspace folder holding `pustman.json` and the fixture environment's file, so the
/// save updates real files rather than creating them.
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

/// The local store the editor saves local values into: one real file-backed database, so
/// a fresh handle can read back what was written (the `selector_tests` shape).
fn open_store(path: &Path) -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open(path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The inputs one open round runs with: `environment`'s scope over `root`, saving through
/// `store`.
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

/// A workspace window with this crate wired up, drawn and active. The Save keystroke is
/// bound here the way `assets/keymaps/` binds every other action's: the binding is what
/// the Save button dispatches the same action through.
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

/// One open round: install `inputs`, dispatch `env_editor::Open`, and hand the editor
/// back once its rows are on screen.
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
fn displayed_rows(editor: &Entity<EnvEditor>, cx: &VisualTestContext) -> Vec<super::DisplayedRow> {
    cx.read(|cx| editor.read_with(cx, |editor, cx| editor.displayed_rows(cx)))
}

/// The row for `variable_key`.
fn row_for<'a>(rows: &'a [super::DisplayedRow], variable_key: &str) -> &'a super::DisplayedRow {
    rows.iter()
        .find(|row| row.key == variable_key)
        .expect("the variable is listed")
}

/// Every string the editor displays, joined — what "the rendered text" means for a leak
/// assertion.
fn displayed_text(rows: &[super::DisplayedRow]) -> String {
    rows.iter()
        .flat_map(|row| {
            [
                Some(row.key.as_str()),
                row.shared.as_deref(),
                row.local.as_deref(),
                Some(row.kind.as_str()),
                Some(row.description.as_str()),
            ]
        })
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

#[gpui::test]
fn secret_row_has_no_shared_value_field(cx: &mut TestAppContext) {
    let environment = staging(vec![
        variable(
            "base_url",
            Some("https://staging.example.test"),
            VariableKind::Default,
        ),
        // The file smuggled a value into a secret (a hand-edited environment): it must
        // reach no field and no display (REQ-VAR-SHARE-02).
        variable("api_token", Some(FAKE_SECRET), VariableKind::Secret),
    ]);
    let root = tempfile::tempdir().expect("the workspace folder opens");
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment, Vec::new()),
    );

    cx.read(|cx| {
        editor.read_with(cx, |editor, _cx| {
            assert!(
                editor.shared_value_field("api_token").is_none(),
                "a secret row renders no shared-value field (REQ-VAR-SHARE-02)"
            );
            assert!(
                editor.local_value_field("api_token").is_none(),
                "a secret row has no local-value field either — its value lives in the OS keychain"
            );
            assert!(
                editor.shared_value_field("base_url").is_some(),
                "an ordinary row's shared value is editable"
            );
        })
    });

    let rows = displayed_rows(&editor, cx);
    let token = row_for(&rows, "api_token");
    assert!(
        token.shared.is_none(),
        "the secret row shows no shared value at all"
    );
    assert!(
        !displayed_text(&rows).contains(FAKE_SECRET),
        "the value a file smuggled into a secret is displayed nowhere"
    );
}

#[gpui::test]
fn shared_value_written_to_file_local_value_to_storage(cx: &mut TestAppContext) {
    let environment = staging(vec![variable(
        "base_url",
        Some("https://staging.example.test"),
        VariableKind::Default,
    )]);
    let scope_id = environment.id.to_string();
    let root = tempfile::tempdir().expect("the workspace folder opens");
    workspace_with(root.path(), &environment);
    let store = open_store(&root.path().join("local.db"));
    let (workspace, cx) = editor_window(cx);
    let editor = open_editor(
        cx,
        &workspace,
        inputs(root.path(), store, environment, Vec::new()),
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(
                editor.set_shared_value("base_url", "https://api.example.test", cx),
                "an ordinary row's shared value is editable"
            );
            assert!(
                editor.set_local_value("base_url", "https://dev.example.test", cx),
                "an ordinary row's local value is editable"
            );
        })
    });
    draw_window(cx);
    cx.simulate_keystrokes("ctrl-s");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        editor_of(&workspace, cx).is_none(),
        "a save that lands closes the editor"
    );

    let file_bytes = std::fs::read(root.path().join("environments/staging.environment.json"))
        .expect("the environment file was written");
    let file: serde_json::Value =
        serde_json::from_slice(&file_bytes).expect("the environment file is JSON");
    assert_eq!(
        file["variables"][0]["value"].as_str(),
        Some("https://api.example.test"),
        "the shared value is saved to the file"
    );
    assert_eq!(
        file["name"].as_str(),
        Some("Staging"),
        "the save rewrites the item, not just its variables"
    );
    assert!(
        !String::from_utf8_lossy(&file_bytes).contains("https://dev.example.test"),
        "a local value never reaches the file (REQ-WSP-06)"
    );

    let fresh = LocalStore::open(root.path().join("local.db")).expect("the store reopens");
    assert_eq!(
        fresh
            .local_values()
            .list(WORKSPACE_ID)
            .expect("the local values read"),
        vec![LocalValue {
            workspace_id: WORKSPACE_ID.to_string(),
            scope_id,
            key: "base_url".to_string(),
            value: "https://dev.example.test".to_string(),
        }],
        "the local value is saved to storage, scoped to this workspace"
    );
}
