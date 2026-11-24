//! The `UnresolvedSecret` panel's door into the editor (S43c): `environment::EditSecret`
//! opens the variables editor on the scope the wire installed with the cursor in the
//! named secret's masked value field — driven through GPUI's action dispatch, against a
//! real workspace folder and a real `storage::LocalStore` (the `env_editor_tests` shape).
//! A name this scope has no secret row for is said out loud, never swallowed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};
use pustman_actions::environment::EditSecret;
use storage::LocalStore;
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{EditTarget, EnvEditor, EnvEditorInputs, NO_SECRET_ROW_MESSAGE, install_inputs};

/// The workspace every fixture's rows belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// One variable of the fixture environment.
fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: None,
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
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The inputs one open round runs with: `environment`'s scope over `root`.
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

/// A workspace window with this crate wired up, drawn and active.
fn editor_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// The editor the workspace's modal layer is showing, if one is open.
fn editor_of(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<Entity<EnvEditor>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<EnvEditor>()
        })
    })
}

/// Whether `name`'s masked value field holds keyboard focus right now.
fn focused_on(editor: &Entity<EnvEditor>, name: &str, cx: &mut VisualTestContext) -> bool {
    let field = cx.read(|cx| editor.read(cx).secret_value_field(name));
    let Some(field) = field else {
        return false;
    };
    cx.update(|window, cx| field.read(cx).focus_handle(cx).is_focused(window))
}

/// How many toasts the workspace is showing.
fn toast_count(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> usize {
    cx.read(|cx| workspace.read_with(cx, |workspace, _| workspace.toasts().read(cx).len()))
}

/// Whether a toast under this debug key is on screen.
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

#[gpui::test]
fn edit_secret_opens_the_editor_focused_on_that_secret(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let environment = staging(vec![
        variable(
            "base_url",
            Some("https://staging.example.test"),
            VariableKind::Default,
        ),
        variable("api_key", None, VariableKind::Secret),
    ]);
    workspace_with(root.path(), &environment);
    let (workspace, cx) = editor_window(cx);
    cx.update(|_window, cx| install_inputs(cx, inputs(root.path(), open_store(), environment)));

    cx.dispatch_action(EditSecret {
        name: String::from("api_key"),
    });
    cx.run_until_parked();
    draw_window(cx);

    let editor = editor_of(&workspace, cx)
        .expect("EditSecret opens the variables editor on the installed scope");
    assert!(
        focused_on(&editor, "api_key", cx),
        "the editor is focused on the secret the action named"
    );
    let rows = cx.read(|cx| editor.read_with(cx, |editor, cx| editor.displayed_rows(cx)));
    assert!(
        rows.iter().any(|row| row.key == "api_key"),
        "the row the cursor landed on is the one the file carries"
    );
}

#[gpui::test]
fn edit_secret_for_a_row_the_scope_does_not_have_says_so(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let environment = staging(vec![variable(
        "base_url",
        Some("https://staging.example.test"),
        VariableKind::Default,
    )]);
    workspace_with(root.path(), &environment);
    let (workspace, cx) = editor_window(cx);
    cx.update(|_window, cx| install_inputs(cx, inputs(root.path(), open_store(), environment)));

    cx.dispatch_action(EditSecret {
        name: String::from("api_key"),
    });
    cx.run_until_parked();
    draw_window(cx);

    let editor = editor_of(&workspace, cx).expect("the editor still opens");
    assert!(
        cx.read(|cx| editor.read(cx).secret_value_field("api_key"))
            .is_none(),
        "this scope has no secret row to focus"
    );
    assert_eq!(toast_count(&workspace, cx), 1, "the refusal is a toast");
    assert!(
        shown(cx, toast_message_selector(NO_SECRET_ROW_MESSAGE)),
        "the toast says the row is not here"
    );
}
