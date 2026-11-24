//! The send each panel answers for (S43c fix 3): a panel's Resend readiness is checked
//! against the send *that panel failed with* — captured when the panel is built — never
//! against whatever send's inputs happen to be installed when the user clicks. Two tabs
//! failing on the same name in different scopes keep their own worlds even though only
//! the last failure's inputs are installed. Also: the display stripping covers every
//! Unicode format range a spoofing name can hide in (AGENTS.md › Security).
//!
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
    EditTarget, EnvEditorInputs, install_inputs as install_editor_inputs,
};
use environment_picker::secret_values::{SecretInputs, install_inputs as install_secret_inputs};
use gpui::{
    AnyView, AnyWindowHandle, AppContext, Entity, Modifiers, TestAppContext, VisualTestContext,
};
use http_engine::{SecretKeyLookup, SendInput, TransportError};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use variables::{ScopeKind, ScopeStack};
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::ResponseView;
use crate::set_secret::{
    RESEND_BUTTON_SELECTOR, display_name, install_inputs as install_set_secret_inputs,
    set_button_selector, set_label, set_tooltip,
};

/// The workspace every fixture's rows and keychain entries belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake value (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";

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

/// The failed send's inputs: `api_key` referenced in the URL, resolving against
/// `environment`'s scope with its own scope's keychain keys.
fn failed_send(environment: &Environment, keychain: &Arc<MemorySecretStore>) -> Arc<SendInput> {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Environment, environment.variables.clone());
    let scope_id = environment.id.to_string();
    let key_of: SecretKeyLookup =
        Arc::new(move |_kind, name: &str| SecretKey::new(WORKSPACE_ID, &scope_id, name).ok());
    Arc::new(SendInput {
        request: HttpRequest {
            id: ItemId::new(),
            name: "fixture request".to_string(),
            method: Method::Get,
            url: String::from("https://api.example.test/?k={{api_key}}"),
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

/// The `UnresolvedSecret` panel for `names`, drawn in `cx`'s window.
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

#[gpui::test]
fn a_panel_answers_with_the_send_it_failed_with(cx: &mut TestAppContext) {
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

    let (_workspace_a, view_a, window_a) = panel_window(cx);
    let (_workspace_b, view_b, window_b) = panel_window(cx);
    let mut cxa = VisualTestContext::from_window(window_a, cx);
    let mut cxb = VisualTestContext::from_window(window_b, cx);

    // Two sends fail in sequence. Their inputs are installed in that order and are
    // never re-installed below: the last failure's — Prod's — stay installed while the
    // user interacts with both panels.
    cx.update(|cx| {
        install_editor_inputs(
            cx,
            editor_inputs(root.path(), &store, &staging, "staging.environment.json"),
        );
        install_set_secret_inputs(cx, failed_send(&staging, &keychain));
    });
    show_panel(&view_a, &["api_key"], &mut cxa);
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
        install_set_secret_inputs(cx, failed_send(&production, &keychain));
    });
    show_panel(&view_b, &["api_key"], &mut cxb);

    // The secret is set only in Staging's scope — where A's send resolves it.
    keychain
        .set(
            &SecretKey::new(WORKSPACE_ID, staging.id.to_string(), "api_key")
                .expect("the key is representable"),
            &SecretValue::new(FAKE_SECRET),
        )
        .expect("the keychain takes the value");

    // A's question must be answered against Staging's send — the one A failed with —
    // even though Prod's inputs are the installed ones.
    cxa.update(|window, _cx| window.activate_window());
    click(&mut cxa, set_button_selector("api_key"));
    press_key(&mut cxa, "escape");
    cxa.run_until_parked();
    assert!(
        shown(&mut cxa, RESEND_BUTTON_SELECTOR.to_owned()),
        "A's panel answers with the send A failed with: Staging resolves `api_key` now"
    );

    // B's question is still Prod's, and asking it leaves A's answer standing.
    cxb.update(|window, _cx| window.activate_window());
    click(&mut cxb, set_button_selector("api_key"));
    press_key(&mut cxb, "escape");
    cxb.run_until_parked();
    assert!(
        !shown(&mut cxb, RESEND_BUTTON_SELECTOR.to_owned()),
        "B's panel still finds nothing in Prod"
    );
    assert!(
        shown(&mut cxa, RESEND_BUTTON_SELECTOR.to_owned()),
        "B's question leaves A's answer standing"
    );
}

#[gpui::test]
fn every_spoofing_format_range_is_stripped_from_shown_text(_cx: &mut TestAppContext) {
    // One character from every range S43c fix 3 adds, beside the ones already covered:
    // U+180E/U+180F, U+200B–U+200F, U+2028–U+202E, U+2060–U+206F, U+FEFF, U+FFF9–U+FFFB
    // and the tag block U+E0000–U+E007F.
    let raw = "a\u{180E}b\u{180F}c\u{200B}d\u{200F}e\u{2028}f\u{202E}g\u{2060}h\u{206A}i\
\u{206F}j\u{FEFF}k\u{FFF9}l\u{FFFB}m\u{E0001}n\u{E007F}o";
    assert_eq!(
        display_name(raw),
        "abcdefghijklmno",
        "every spoofing format character is stripped"
    );
    assert_eq!(set_label(raw), "Set abcdefghijklmno");
    assert_eq!(
        set_tooltip(raw),
        "Set abcdefghijklmno's secret value in the variables editor"
    );
    assert_eq!(
        set_button_selector(&display_name(raw)),
        "set-secret-button-abcdefghijklmno"
    );
}
