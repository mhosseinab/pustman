//! Per-panel answers and display-safe text in the `UnresolvedSecret` panel (S43c fix 2):
//! every panel's Resend answer belongs to that panel's own question — its own missing
//! names under the send *it* failed with — so two panels failing on the same name in
//! different scopes never share or withdraw each other's answers. What the panel shows is
//! display-safe everywhere: the headline, the buttons and the tooltip carry no control,
//! bidi or zero-width characters, and two names that differ only in stripped characters
//! keep their own buttons and their own rows (AGENTS.md › Security).
//!
//! Two windows stand in for two request tabs: each holds its own response region, its own
//! panel, and its own `EditSecret` dispatch (`VisualTestContext::from_window`).
//! `MemorySecretStore` is the keychain; the fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, Method, Variable, VariableKind,
    Workspace as WorkspaceModel,
};
use environment_picker::env_editor::{
    EditTarget, EnvEditor, EnvEditorInputs, Save, install_inputs as install_editor_inputs,
};
use environment_picker::secret_values::{SecretInputs, install_inputs as install_secret_inputs};
use gpui::{
    Action, AnyView, AnyWindowHandle, AppContext, Entity, FocusHandle, Focusable, Modifiers,
    TestAppContext, VisualTestContext,
};
use http_engine::{SecretKeyLookup, SendInput, TransportError};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use variables::{ScopeKind, ScopeStack};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::ResponseView;
use crate::error_panel::error_message_selector;
use crate::set_secret::{
    PREPARE_FAILED_NOTE, RESEND_BUTTON_SELECTOR, install_inputs as install_set_secret_inputs,
    pending_note_selector, set_button_selector,
};

/// The workspace every fixture's rows and keychain entries belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake values (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";

/// A name carrying a bidi override and a zero-width space: it displays as `apikey` and
/// must reach no label, headline or tooltip with its invisibles (fix 2).
const CRAFTED_NAME: &str = "api\u{202E}\u{200B}key";

/// The name as the panel must show it.
const CRAFTED_DISPLAY: &str = "apikey";

/// One secret variable of a fixture scope: a key and a kind, no value.
fn secret_variable(key: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: None,
        kind: VariableKind::Secret,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// One ordinary variable with a shared value.
fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// A fixture environment.
fn environment(name: &str, variables: Vec<Variable>) -> Environment {
    Environment {
        id: ItemId::new(),
        name: name.to_string(),
        variables,
        extra: BTreeMap::new(),
    }
}

/// The workspace item an environment's shared values are saved to — one file per scope.
fn environment_item(environment: &Environment, file_name: &str) -> Item {
    Item {
        body: ItemBody::Environment(environment.clone()),
        dir: PathBuf::from("environments"),
        path: Some(PathBuf::from(format!("environments/{file_name}"))),
    }
}

/// A workspace folder holding `pustman.json` and every fixture environment's file.
fn workspace_with(root: &Path, environments: &[(&str, &Environment)]) {
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
    for (file_name, environment) in environments {
        std::fs::write(
            root.join("environments").join(file_name),
            serde_json::to_vec_pretty(environment).expect("the environment file serializes"),
        )
        .expect("the environment file is written");
    }
}

/// The local store the editor saves local values into.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The editor inputs for `environment`'s scope over `root`, saving to its own file.
fn editor_inputs(
    root: &Path,
    store: &Arc<Mutex<LocalStore>>,
    environment: &Environment,
    file_name: &str,
) -> EnvEditorInputs {
    EnvEditorInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        workspace_root: root.to_path_buf(),
        target: EditTarget::new(environment_item(environment, file_name))
            .expect("an environment is editable"),
        local_values: Vec::new(),
        store: Arc::clone(store),
    }
}

/// One scope's keychain-key mapping: every name under `scope_id`.
fn key_lookup(scope_id: &str) -> SecretKeyLookup {
    let scope_id = scope_id.to_string();
    Arc::new(move |_kind, name: &str| SecretKey::new(WORKSPACE_ID, &scope_id, name).ok())
}

/// The failed send's inputs: `url`'s placeholders resolving against `environment`'s scope
/// with `scope_id`'s keychain keys.
fn failed_send(
    url: &str,
    environment: &Environment,
    scope_id: &str,
    keychain: &Arc<MemorySecretStore>,
) -> Arc<SendInput> {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Environment, environment.variables.clone());
    Arc::new(SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "fixture request".to_string(),
            method: Method::Get,
            url: url.to_string(),
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
        secret_key: key_lookup(scope_id),
    })
}

/// A workspace window with a response region in its center pane.
fn panel_window(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, Entity<ResponseView>, AnyWindowHandle) {
    let window = cx.add_window(Workspace::new);
    let workspace = window.root(cx).expect("the workspace root");
    window
        .update(cx, |_workspace, window, _cx| window.activate_window())
        .expect("the workspace window is open");
    let view = cx.new(ResponseView::new);
    cx.update(|cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.center_pane().update(cx, |pane, cx| {
                pane.add_item(AnyView::from(view.clone()), cx);
            });
        });
    });
    (workspace, view, *window.deref())
}

/// The `UnresolvedSecret` panel for `names`, drawn in `window`.
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

/// Whether anything in `cx`'s window is on screen under this debug key.
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

/// Click the control on screen under `selector` the way a user clicks it.
fn click(cx: &mut VisualTestContext, selector: String) {
    let bounds = cx
        .debug_bounds(selector.leak())
        .expect("the control is on screen");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

/// The variables editor a window's modal layer is showing, if one is open.
fn editor_of(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Option<Entity<EnvEditor>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<EnvEditor>()
        })
    })
}

/// Which of `names` the open editor holds keyboard focus on.
fn focused_secret(
    editor: &Entity<EnvEditor>,
    names: &[&str],
    cx: &mut VisualTestContext,
) -> Option<String> {
    names.iter().find_map(|name| {
        let focused = cx.update(|window, cx| {
            editor
                .read(cx)
                .secret_value_field(name)
                .is_some_and(|field| field.read(cx).focus_handle(cx).is_focused(window))
        });
        focused.then(|| name.to_string())
    })
}

/// The keyboard focus handle right now.
fn focused_handle(cx: &mut VisualTestContext) -> Option<FocusHandle> {
    cx.update(|window, cx| window.focused(cx))
}

/// Save the open editor's rows and let everything land.
fn save_editor(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Save.boxed_clone(), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn panels_in_different_scopes_keep_their_own_resend_answers(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = environment("Staging", vec![secret_variable("api_key")]);
    let production = environment("Prod", vec![secret_variable("api_key")]);
    workspace_with(
        root.path(),
        &[
            ("staging.environment.json", &staging),
            ("production.environment.json", &production),
        ],
    );
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
    });

    // Two request tabs: one failed needing `api_key` in Staging, the other in Prod.
    let (workspace_a, view_a, window_a) = panel_window(cx);
    let (_workspace_b, view_b, window_b) = panel_window(cx);
    let mut cxa = VisualTestContext::from_window(window_a, cx);
    let mut cxb = VisualTestContext::from_window(window_b, cx);
    cxa.update(|window, _cx| window.activate_window());
    draw_window(&mut cxa);

    // Prod's world: tab B's panel and its own send. B asks first — nothing is set in
    // Prod, so its own answer is "still missing". The send Arcs are kept: reinstalling
    // the same send keeps its question identity (module docs).
    let production_send = failed_send(
        "https://api.example.test/?k={{api_key}}",
        &production,
        &production.id.to_string(),
        &keychain,
    );
    let staging_send = failed_send(
        "https://api.example.test/?k={{api_key}}",
        &staging,
        &staging.id.to_string(),
        &keychain,
    );
    cx.update(|cx| {
        install_editor_inputs(
            cx,
            editor_inputs(
                root.path(),
                &store,
                &production,
                "production.environment.json",
            ),
        );
        install_set_secret_inputs(cx, Arc::clone(&production_send));
    });
    show_panel(&view_b, &["api_key"], &mut cxb);
    cxb.update(|window, _cx| window.activate_window());
    cxb.update(|window, _cx| window.activate_window());
    click(&mut cxb, set_button_selector("api_key"));
    press_key(&mut cxb, "escape");
    cxb.run_until_parked();

    // Staging's world: tab A's panel and send. The user sets `api_key` where A needs it.
    cx.update(|cx| {
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
        install_set_secret_inputs(cx, Arc::clone(&staging_send));
    });
    show_panel(&view_a, &["api_key"], &mut cxa);
    cxa.update(|window, _cx| window.activate_window());
    click(&mut cxa, set_button_selector("api_key"));
    let editor = editor_of(&workspace_a, &cxa).expect("A's Set button opens the editor");
    editor.update(&mut cxa, |editor, cx| {
        assert!(editor.set_secret_value("api_key", FAKE_SECRET, cx));
    });
    save_editor(&mut cxa);

    // Set only in A's scope: A offers Resend, B does not.
    assert!(
        shown(&mut cxa, RESEND_BUTTON_SELECTOR.to_owned()),
        "A's panel offers Resend for the scope A's secret was set in"
    );
    assert!(
        !shown(&mut cxb, RESEND_BUTTON_SELECTOR.to_owned()),
        "B's panel keeps its own answer: nothing is set in Prod"
    );

    // Clicking Set on B must leave A's answer alone.
    cx.update(|cx| {
        install_editor_inputs(
            cx,
            editor_inputs(
                root.path(),
                &store,
                &production,
                "production.environment.json",
            ),
        );
        install_set_secret_inputs(cx, Arc::clone(&production_send));
    });
    cxb.update(|window, _cx| window.activate_window());
    click(&mut cxb, set_button_selector("api_key"));
    press_key(&mut cxb, "escape");
    cxb.run_until_parked();
    assert!(
        !shown(&mut cxb, RESEND_BUTTON_SELECTOR.to_owned()),
        "B still resolves nothing in Prod"
    );
    cx.update(|cx| {
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
        install_set_secret_inputs(cx, Arc::clone(&staging_send));
    });
    draw_window(&mut cxa);
    assert!(
        shown(&mut cxa, RESEND_BUTTON_SELECTOR.to_owned()),
        "clicking Set on B left A's answer standing"
    );
}

#[gpui::test]
fn a_crafted_name_is_stripped_from_the_headline_buttons_and_tooltip(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = environment("Staging", vec![secret_variable(CRAFTED_NAME)]);
    workspace_with(root.path(), &[("staging.environment.json", &staging)]);
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
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
    });

    let (_workspace, view, handle) = panel_window(cx);
    let mut cx = VisualTestContext::from_window(handle, cx);
    show_panel(&view, &[CRAFTED_NAME], &mut cx);

    let error = TransportError::UnresolvedSecret {
        names: vec![CRAFTED_NAME.to_string()],
    };
    assert!(
        shown(
            &mut cx,
            error_message_selector(&crate::set_secret::display_name(&error.user_message()))
        ),
        "the headline shows the name without its invisibles"
    );
    assert!(
        !shown(&mut cx, error_message_selector(&error.user_message())),
        "the raw name — bidi override and all — reaches no headline"
    );
    assert!(
        shown(&mut cx, set_button_selector(CRAFTED_DISPLAY)),
        "the button label is the name without its invisibles"
    );
    let tooltip = crate::set_secret::set_tooltip(CRAFTED_NAME);
    assert!(
        !tooltip.contains('\u{202E}') && !tooltip.contains('\u{200B}'),
        "the tooltip names the secret without its invisibles: {tooltip:?}"
    );
}

#[gpui::test]
fn names_that_differ_only_in_invisibles_keep_their_own_buttons(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    let staging = environment(
        "Staging",
        vec![secret_variable("apikey"), secret_variable(CRAFTED_NAME)],
    );
    workspace_with(root.path(), &[("staging.environment.json", &staging)]);
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
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
    });

    let (workspace, view, handle) = panel_window(cx);
    let mut cx = VisualTestContext::from_window(handle, cx);
    // Both names display as `apikey`: the two buttons look the same but must be two
    // controls. The shared debug selector resolves to the last-painted one, so the row is
    // rendered in both orders to reach each in turn.
    show_panel(&view, &["apikey", CRAFTED_NAME], &mut cx);
    click(&mut cx, set_button_selector(CRAFTED_DISPLAY));
    let editor = editor_of(&workspace, &cx).expect("the Set button opens the editor");
    assert_eq!(
        focused_secret(&editor, &["apikey", CRAFTED_NAME], &mut cx).as_deref(),
        Some(CRAFTED_NAME),
        "the second button aims at the second name's row"
    );
    press_key(&mut cx, "escape");
    cx.run_until_parked();
    let first_button_focus = focused_handle(&mut cx);

    show_panel(&view, &[CRAFTED_NAME, "apikey"], &mut cx);
    click(&mut cx, set_button_selector(CRAFTED_DISPLAY));
    let editor = editor_of(&workspace, &cx).expect("the Set button opens the editor");
    assert_eq!(
        focused_secret(&editor, &["apikey", CRAFTED_NAME], &mut cx).as_deref(),
        Some("apikey"),
        "the other button aims at the other name's row"
    );
    press_key(&mut cx, "escape");
    cx.run_until_parked();
    let second_button_focus = focused_handle(&mut cx);

    assert!(
        first_button_focus.is_some() && first_button_focus != second_button_focus,
        "two names differing only in stripped characters get their own controls"
    );
}

#[gpui::test]
fn a_request_that_does_not_prepare_cleanly_offers_no_resend(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().expect("the fixture workspace has a folder");
    // `a` resolves to `{{b}}` and `b` back to `{{a}}`: this request cannot prepare at all.
    let staging = environment(
        "Staging",
        vec![
            secret_variable("api_key"),
            variable("a", "{{b}}"),
            variable("b", "{{a}}"),
        ],
    );
    workspace_with(root.path(), &[("staging.environment.json", &staging)]);
    let store = open_store();
    let keychain = Arc::new(MemorySecretStore::new());
    keychain
        .set(
            &SecretKey::new(WORKSPACE_ID, staging.id.to_string(), "api_key")
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
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
        install_set_secret_inputs(
            cx,
            failed_send(
                "https://api.example.test/?k={{api_key}}&x={{a}}",
                &staging,
                &staging.id.to_string(),
                &keychain,
            ),
        );
    });

    let (_workspace, view, handle) = panel_window(cx);
    let mut cx = VisualTestContext::from_window(handle, cx);
    show_panel(&view, &["api_key"], &mut cx);
    click(&mut cx, set_button_selector("api_key"));
    press_key(&mut cx, "escape");
    cx.run_until_parked();

    assert!(
        !shown(&mut cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "a request that does not prepare cleanly offers no Resend — it would fail again"
    );
    assert!(
        shown(&mut cx, pending_note_selector(PREPARE_FAILED_NOTE)),
        "the panel says why Resend is not offered"
    );
}
