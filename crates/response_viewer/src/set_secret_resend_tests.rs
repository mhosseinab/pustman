//! What the `UnresolvedSecret` panel offers Resend for (S43c fix 1): Resend's readiness is
//! the *send's* answer, not the editor's — the re-check runs the same
//! `http_engine::prepare` resolution the send runs, over the scopes the send resolves
//! against, and Resend is offered only when every name the failed send could not resolve
//! resolves now. A secret set in one scope must not unblock a failure that needs it in
//! another; a disabled secret's value must not unblock anything; a panel that cannot
//! re-check says so instead of withholding silently. Also: an unwired editor refuses
//! visibly without wedging the button, and a crafted variable name never reaches a label
//! (AGENTS.md › Security).
//!
//! The keychain is a [`MemorySecretStore`] and the fixture values are obviously fake
//! (`.agents/rules/tests.md`). Where a test "sets" a secret directly in the keychain it
//! says so: the behavior under test is what the re-check can *see*, and a name owned by a
//! scope the editor is not open on is set exactly where the send would read it.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, Method, Variable, VariableKind,
    Workspace as WorkspaceModel,
};
use environment_picker::env_editor::{
    EditTarget, EnvEditor, EnvEditorInputs, Open as OpenEditor, Save,
    install_inputs as install_editor_inputs,
};
use environment_picker::secret_values::{SecretInputs, install_inputs as install_secret_inputs};
use gpui::{
    Action, AnyView, AppContext, Entity, Focusable, Modifiers, TestAppContext, VisualTestContext,
};
use http_engine::{SecretKeyLookup, SendInput, TransportError};
use pustman_actions::environment::EditSecret;
use pustman_actions::request::Send as SendRequest;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use variables::{ScopeKind, ScopeStack};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::ResponseView;
use crate::set_secret::{
    NO_RESOLUTION_NOTE, RESEND_BUTTON_SELECTOR, install_inputs as install_set_secret_inputs,
    missing_note, pending_note_selector, set_button_selector,
};

/// The workspace every fixture's rows and keychain entries belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// The scope id globals' keychain entries live under — the id the variables editor writes
/// globals under (`EditTarget::scope_id`), read back through a fixture `EditTarget`.
const GLOBALS_SCOPE: &str = "globals";

/// Obviously fake values (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";
const FAKE_SECRET_TWO: &str = "token-REDACTED-other";

/// A name carrying a bidi override: invisible in a label, and stripped from every label
/// the panel draws (`display_name`).
const CRAFTED_NAME: &str = "api\u{202E}key";

/// One variable of a fixture scope: a key and a kind, no value — the file never carries a
/// secret's (REQ-VAR-SHARE-02).
fn secret_variable(key: &str, enabled: bool) -> Variable {
    Variable {
        key: key.to_string(),
        value: None,
        kind: VariableKind::Secret,
        enabled,
        description: None,
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
/// editor's save updates real files rather than creating them.
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

/// The editor inputs for `environment`'s scope over `root`.
fn editor_inputs(
    root: &Path,
    store: &Arc<Mutex<LocalStore>>,
    environment: &Environment,
) -> EnvEditorInputs {
    EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target: EditTarget::new(environment_item(environment)).expect("an environment is editable"),
        local_values: Vec::new(),
        store: Arc::clone(store),
    }
}

/// The failed send's inputs: `names` referenced in the URL, resolving against `scopes`
/// with `key_of`'s keychain keys — the shape [`SendInput`] carries to `prepare`.
fn failed_send(
    names: &[&str],
    scopes: ScopeStack,
    keychain: &Arc<MemorySecretStore>,
    key_of: SecretKeyLookup,
) -> Arc<SendInput> {
    let placeholders = names
        .iter()
        .map(|name| format!("{{{{{name}}}}}"))
        .collect::<Vec<_>>()
        .join("&");
    Arc::new(SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "fixture request".to_string(),
            method: Method::Get,
            url: format!("https://api.example.test/?{placeholders}"),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        },
        ancestors: Vec::new(),
        scopes,
        secrets: Arc::clone(keychain) as Arc<dyn SecretStore>,
        secret_key: key_of,
    })
}

/// A workspace window drawn and active, with the response region under its center pane.
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

/// The `UnresolvedSecret` panel for `names`, drawn.
fn show_panel(view: &Entity<ResponseView>, names: &[&str], cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.set_error(
            TransportError::UnresolvedSecret {
                names: names.iter().map(|name| name.to_string()).collect(),
            },
            cx,
        )
    });
    draw_window(cx);
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: the selector is leaked — fine for a test's short life).
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

/// The variables editor the workspace's modal layer is showing, if one is open.
fn editor_of(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<Entity<EnvEditor>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<EnvEditor>()
        })
    })
}

/// Whether `name`'s masked field in `editor` holds keyboard focus right now.
fn focused_on(editor: &Entity<EnvEditor>, name: &str, cx: &mut VisualTestContext) -> bool {
    let field = cx.read(|cx| editor.read(cx).secret_value_field(name));
    let Some(field) = field else {
        return false;
    };
    cx.update(|window, cx| field.read(cx).focus_handle(cx).is_focused(window))
}

/// Save the open editor's rows and let everything land.
fn save_editor(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Save.boxed_clone(), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn setting_a_secret_in_one_scope_does_not_offer_resend_for_another(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable("api_key", true)],
        extra: BTreeMap::new(),
    };
    let production = Environment {
        id: ItemId::new(),
        name: "Prod".to_string(),
        variables: vec![secret_variable("api_key", true)],
        extra: BTreeMap::new(),
    };
    workspace_with(root.path(), &staging);
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());
    let sent = Rc::new(Cell::new(false));

    // Staging's world: `api_key` lives in the environment scope the send resolves against.
    let staging_scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, staging.variables.clone());
        scopes
    };
    let staging_id = staging.id.to_string();
    let staging_key: SecretKeyLookup =
        Arc::new(move |_kind, name: &str| SecretKey::new(WORKSPACE_ID, &staging_id, name).ok());

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
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &staging));
        install_set_secret_inputs(
            cx,
            failed_send(&["api_key"], staging_scopes, &keychain, staging_key),
        );
        let sent = Rc::clone(&sent);
        cx.on_action(move |_: &SendRequest, _cx| sent.set(true));
    });

    let (workspace, view, cx) = panel_window(cx);
    show_panel(&view, &["api_key"], cx);

    // The user sets `api_key` for Staging the way the button leads them to.
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    let editor = editor_of(&workspace, cx).expect("the Set button opens the variables editor");
    assert!(
        focused_on(&editor, "api_key", cx),
        "the editor is focused on that secret"
    );
    editor.update(cx, |editor, cx| {
        assert!(editor.set_secret_value("api_key", FAKE_SECRET, cx));
    });
    save_editor(cx);
    assert!(
        shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "set for the scope that needs it, the panel offers Resend"
    );

    // A later failure needs `api_key` in Prod — where nothing is set yet.
    let production_scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, production.variables.clone());
        scopes
    };
    let production_id = production.id.to_string();
    let production_key: SecretKeyLookup =
        Arc::new(move |_kind, name: &str| SecretKey::new(WORKSPACE_ID, &production_id, name).ok());
    cx.update(|_window, cx| {
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &production));
        install_set_secret_inputs(
            cx,
            failed_send(&["api_key"], production_scopes, &keychain, production_key),
        );
    });
    show_panel(&view, &["api_key"], cx);

    assert!(
        !shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "a failure that needs the secret in another scope offers no Resend before any save"
    );
}

#[gpui::test]
fn resend_waits_for_every_missing_secret_whatever_scope_owns_it(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable("api_key", true)],
        extra: BTreeMap::new(),
    };
    workspace_with(root.path(), &staging);
    let staging_id = staging.id.to_string();
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());
    let sent = Rc::new(Cell::new(false));

    // `api_key` belongs to the environment scope, `token` to the globals — the send
    // resolves each where the winning enabled binding lives.
    let scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, staging.variables.clone());
        scopes.push(ScopeKind::Global, vec![secret_variable("token", true)]);
        scopes
    };
    let key_of: SecretKeyLookup = Arc::new(move |kind, name: &str| {
        let scope_id = match kind {
            ScopeKind::Global => GLOBALS_SCOPE,
            _ => &staging_id,
        };
        SecretKey::new(WORKSPACE_ID, scope_id, name).ok()
    });

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
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &staging));
        install_set_secret_inputs(
            cx,
            failed_send(&["api_key", "token"], scopes, &keychain, key_of),
        );
        let sent = Rc::clone(&sent);
        cx.on_action(move |_: &SendRequest, _cx| sent.set(true));
    });

    let (workspace, view, cx) = panel_window(cx);
    show_panel(&view, &["api_key", "token"], cx);

    // One of the two is set — through the editor, its own scope.
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    let editor = editor_of(&workspace, cx).expect("the Set button opens the variables editor");
    editor.update(cx, |editor, cx| {
        assert!(editor.set_secret_value("api_key", FAKE_SECRET, cx));
    });
    save_editor(cx);
    assert!(
        !shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "one secret set of two offers no Resend"
    );

    // The other one is set too — under the globals key the send reads it from (the
    // editor here is installed on the environment, as it is in the app).
    keychain
        .set(
            &SecretKey::new(WORKSPACE_ID, GLOBALS_SCOPE, "token")
                .expect("the key is representable"),
            &SecretValue::new(FAKE_SECRET_TWO),
        )
        .expect("the keychain takes the value");
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    save_editor(cx);

    assert!(
        shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "both secrets set — in whatever scope owns each — the panel offers Resend"
    );
    click(cx, RESEND_BUTTON_SELECTOR.to_owned());
    assert!(sent.get(), "Resend re-sends the request");
}

#[gpui::test]
fn a_disabled_secrets_value_does_not_unblock_resend(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    // The environment's `api_key` is disabled and has a value; the enabled globals one —
    // the binding the send actually resolves — has none.
    let staging = Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable("api_key", false)],
        extra: BTreeMap::new(),
    };
    workspace_with(root.path(), &staging);
    let staging_id = staging.id.to_string();
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());

    let scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, staging.variables.clone());
        scopes.push(ScopeKind::Global, vec![secret_variable("api_key", true)]);
        scopes
    };
    let key_staging_id = staging_id.clone();
    let key_of: SecretKeyLookup = Arc::new(move |kind, name: &str| {
        let scope_id = match kind {
            ScopeKind::Global => GLOBALS_SCOPE,
            _ => &key_staging_id,
        };
        SecretKey::new(WORKSPACE_ID, scope_id, name).ok()
    });

    // The disabled secret's value — set exactly where the editor would write it.
    keychain
        .set(
            &SecretKey::new(WORKSPACE_ID, &staging_id, "api_key")
                .expect("the key is representable"),
            &SecretValue::new(FAKE_SECRET),
        )
        .expect("the keychain takes the value");

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
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &staging));
        install_set_secret_inputs(cx, failed_send(&["api_key"], scopes, &keychain, key_of));
    });

    let (workspace, view, cx) = panel_window(cx);
    show_panel(&view, &["api_key"], cx);

    // The user goes through the Set flow and saves: what the editor writes is the
    // disabled secret's own value — the one the send cannot use.
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    editor_of(&workspace, cx).expect("the Set button opens the variables editor");
    save_editor(cx);

    assert!(
        !shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "a disabled secret's value offers no Resend — the send still resolves nothing"
    );
    assert!(
        shown(
            cx,
            pending_note_selector(&missing_note(&["api_key".to_string()]))
        ),
        "the panel says why Resend is not offered"
    );
}

#[gpui::test]
fn unwired_set_refuses_visibly_and_keeps_the_button_working(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable("api_key", true)],
        extra: BTreeMap::new(),
    };
    workspace_with(root.path(), &staging);
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());

    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        environment_picker::init(cx);
    });

    let (workspace, view, cx) = panel_window(cx);
    show_panel(&view, &["api_key"], cx);

    // Nothing is wired yet (S43w): the click refuses visibly and offers no Resend, and
    // the re-check says why instead of returning silently.
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    let toasts =
        cx.read(|cx| workspace.read_with(cx, |workspace, _| workspace.toasts().read(cx).len()));
    assert!(
        toasts == 1,
        "the editor's refusal toast is shown ({toasts} toasts)"
    );
    assert!(
        shown(cx, pending_note_selector(NO_RESOLUTION_NOTE)),
        "the re-check says why Resend is not offered"
    );
    assert!(!shown(cx, RESEND_BUTTON_SELECTOR.to_owned()));

    // Once wired, the button works — and an editor opened some other way is never
    // dragged to a row a refused click named (there is no aim left behind), while the
    // action arriving again aims it instead of toggling it shut.
    cx.update(|_window, cx| {
        install_secret_inputs(
            cx,
            SecretInputs {
                store: Arc::clone(&keychain) as Arc<dyn SecretStore>,
            },
        );
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &staging));
    });
    cx.update(|window, cx| window.dispatch_action(OpenEditor.boxed_clone(), cx));
    cx.run_until_parked();
    let editor = editor_of(&workspace, cx).expect("the editor opens");
    assert!(
        !focused_on(&editor, "api_key", cx),
        "a refused click leaves no aim behind"
    );
    cx.update(|window, cx| {
        window.dispatch_action(
            EditSecret {
                name: String::from("api_key"),
            }
            .boxed_clone(),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(
        focused_on(&editor, "api_key", cx),
        "the action aims the editor that is already open, instead of toggling it shut"
    );

    press_key(cx, "escape");
    cx.run_until_parked();
    click(cx, set_button_selector("api_key"));
    cx.run_until_parked();
    let editor = editor_of(&workspace, cx).expect("the Set button opens the variables editor");
    assert!(
        focused_on(&editor, "api_key", cx),
        "the button aims the editor at the secret again"
    );
}

#[gpui::test]
fn a_crafted_secret_name_never_reaches_the_labels(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![secret_variable(CRAFTED_NAME, true)],
        extra: BTreeMap::new(),
    };
    workspace_with(root.path(), &staging);
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());

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
        install_editor_inputs(cx, editor_inputs(root.path(), &store, &staging));
    });

    let (workspace, view, cx) = panel_window(cx);
    show_panel(&view, &[CRAFTED_NAME], cx);

    assert!(
        shown(cx, set_button_selector("apikey")),
        "the label shows the name without its bidi override"
    );
    assert!(
        !shown(cx, set_button_selector(CRAFTED_NAME)),
        "the crafted name itself reaches no label"
    );

    // The lookup keeps the name as it is: the button still opens the editor on the row
    // the file actually carries.
    click(cx, set_button_selector("apikey"));
    cx.run_until_parked();
    let editor = editor_of(&workspace, cx).expect("the Set button opens the variables editor");
    assert!(
        focused_on(&editor, CRAFTED_NAME, cx),
        "the editor is focused on the secret under its real name"
    );
}
