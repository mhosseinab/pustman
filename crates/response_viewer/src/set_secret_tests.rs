//! The `UnresolvedSecret` panel's "Set <name>" flow, driven the way a user drives it:
//! the buttons in the panel, the variables editor they open focused on the secret, the
//! save that lands the value in the OS keychain, and the Resend button the panel offers
//! once every missing name is set — against a real workspace folder on disk, a real
//! `storage::LocalStore` and a [`MemorySecretStore`] standing in for the OS keychain. A
//! flow that parked the value anywhere but the keychain, or offered Resend before it was
//! set, fails these assertions (AGENTS.md › Security).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, Variable, VariableKind, Workspace as WorkspaceModel};
use environment_picker::env_editor::{
    EditTarget, EnvEditor, EnvEditorInputs, Save, install_inputs as install_editor_inputs,
};
use environment_picker::secret_values::{SecretInputs, install_inputs as install_secret_inputs};
use gpui::{
    Action, AnyView, AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext,
};
use http_engine::TransportError;
use pustman_actions::request::Send as SendRequest;
use secrets::{MemorySecretStore, SecretKey, SecretStore};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use super::{RESEND_BUTTON_SELECTOR, set_button_selector};
use crate::ResponseView;

/// The workspace every fixture's rows and keychain entries belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// The two secrets the fixture request references and the keychain holds nothing for.
const API_KEY: &str = "api_key";
const TOKEN: &str = "token";

/// The fixture values — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";
const FAKE_SECRET_TWO: &str = "token-REDACTED-other";

/// One secret variable of the fixture environment: a key and a kind, no value — the file
/// never carries one (REQ-VAR-SHARE-02).
fn secret_variable(key: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: None,
        kind: VariableKind::Secret,
        enabled: true,
        description: Some("a credential the API requires".to_string()),
        extra: BTreeMap::new(),
    }
}

/// The environment the editor opens on.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable(API_KEY), secret_variable(TOKEN)],
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
/// save updates real files rather than creating them (the `env_editor_tests` shape).
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

/// A workspace window drawn and active, with the response region under its center pane —
/// the shell the Set button opens the editor in.
fn panel_window(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ResponseView>,
    &mut VisualTestContext,
) {
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    let view = cx.new(ResponseView::new);
    cx.update(|_window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.center_pane().update(cx, |pane, cx| {
                pane.add_item(AnyView::from(view.clone()), cx);
            });
        });
    });
    draw_window(cx);
    (workspace, view, cx)
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

/// Click the control on screen under `selector` the way a user clicks it.
fn click(cx: &mut VisualTestContext, selector: String) {
    let bounds = cx
        .debug_bounds(selector.leak())
        .expect("the control is on screen");
    cx.simulate_click(bounds.center(), Modifiers::none());
}

#[gpui::test]
fn unresolved_secret_panel_offers_set_and_resend(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let environment = staging();
    workspace_with(root.path(), &environment);
    let scope_id = environment.id.to_string();
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());
    let sent = Rc::new(Cell::new(false));

    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        environment_picker::init(cx);
        install_secret_inputs(
            cx,
            SecretInputs {
                store: Arc::clone(&keychain) as Arc<dyn SecretStore>,
            },
        );
        install_editor_inputs(
            cx,
            EnvEditorInputs {
                workspace_id: WORKSPACE_ID.to_string(),
                workspace_root: root.path().to_path_buf(),
                target: EditTarget::new(environment_item(&environment))
                    .expect("an environment is editable"),
                local_values: Vec::new(),
                store: Arc::clone(&store),
            },
        );
        let sent = Rc::clone(&sent);
        cx.on_action(move |_: &SendRequest, _cx| sent.set(true));
    });

    let (workspace, view, cx) = panel_window(cx);

    // A send that failed because both secrets it references have no value at all.
    view.update(cx, |view, cx| {
        view.set_error(
            TransportError::UnresolvedSecret {
                names: vec![API_KEY.to_string(), TOKEN.to_string()],
            },
            cx,
        )
    });
    draw_window(cx);

    assert!(
        shown(cx, set_button_selector(API_KEY)),
        "the panel offers Set for the missing `api_key`"
    );
    assert!(
        shown(cx, set_button_selector(TOKEN)),
        "the panel offers Set for every missing name, `token` included"
    );
    assert!(
        !shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "Resend is offered on save — re-sending before anything is set would fail again"
    );

    // The user clicks "Set `api_key`" and lands in the variables editor with that
    // secret's masked field under the cursor.
    click(cx, set_button_selector(API_KEY));
    cx.run_until_parked();
    let editor = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace.modal_layer().active_modal::<EnvEditor>()
            })
        })
        .expect("the Set button opens the variables editor");
    let field = cx.read(|cx| {
        editor
            .read(cx)
            .secret_value_field(API_KEY)
            .expect("the editor has the `api_key` row")
    });
    assert!(
        cx.update(|window, cx| field.read(cx).focus_handle(cx).is_focused(window)),
        "the editor is focused on that secret"
    );

    // The user types both values — into the editor's masked fields, the only place this
    // flow ever puts one — and saves.
    editor.update(cx, |editor, cx| {
        assert!(editor.set_secret_value(API_KEY, FAKE_SECRET, cx));
        assert!(editor.set_secret_value(TOKEN, FAKE_SECRET_TWO, cx));
    });
    cx.update(|window, cx| window.dispatch_action(Save.boxed_clone(), cx));
    cx.run_until_parked();

    // The values are in the keychain, each under its own key — and nowhere else.
    for (name, value) in [(API_KEY, FAKE_SECRET), (TOKEN, FAKE_SECRET_TWO)] {
        let key = SecretKey::new(WORKSPACE_ID, &scope_id, name).expect("the key is representable");
        let stored = keychain.get(&key).expect("the keychain has the value");
        assert_eq!(stored.expose(), value, "the value landed under its own key");
    }
    let file = std::fs::read_to_string(root.path().join("environments/staging.environment.json"))
        .expect("the environment file is readable");
    assert!(
        !file.contains(FAKE_SECRET) && !file.contains(FAKE_SECRET_TWO),
        "the shared file never holds a secret value"
    );
    let locals = store
        .lock()
        .expect("the store is not poisoned")
        .local_values()
        .list(WORKSPACE_ID)
        .expect("the local values read");
    assert!(
        locals
            .iter()
            .all(|row| row.value != FAKE_SECRET && row.value != FAKE_SECRET_TWO),
        "the local store never holds a secret value"
    );

    // On save the panel offers Resend — and clicking it is a re-send.
    draw_window(cx);
    assert!(
        shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "on save the panel offers Resend"
    );
    click(cx, RESEND_BUTTON_SELECTOR.to_owned());
    assert!(sent.get(), "Resend re-sends the request");
}
