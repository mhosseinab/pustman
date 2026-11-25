//! The unresolved-secret panel in the running app (S43x, from #324): when a send fails
//! as `UnresolvedSecret`, the response panel's "Set <name>" buttons and its Resend offer
//! work end to end — the failed send's own inputs are what the panel captures, the value
//! is set through the variables editor's own save path, and Resend's send carries it.
//!
//! Every test boots through the real `pustman::init_with_store` over an in-memory store,
//! opens a temp workspace folder through `WorkspaceOpen`, and replaces the app's OS
//! keychain with a `MemorySecretStore` — the real keychain is never touched (the
//! `wire_scope_wiring` shape). The sends go through a `FakeTransport` client
//! (`.agents/rules/tests.md` forbids real network in GPUI tests) and the transport
//! records the resolved request exactly as a server would see it. Every value is
//! obviously fake.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{
    Auth, Body, Collection, Environment, HttpRequest, ItemId, KeyValue, Method, Request, Variable,
    VariableKind,
};
use environment_picker::env_editor::{EnvEditor, Save as SaveVariables};
use environment_picker::secret_values::{self, SecretInputs};
use gpui::{Action, AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, ResolvedRequest, TransportError};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use pustman_actions::quick_open::Toggle as ToggleQuickOpen;
use request_editor::RequestEditor;
use request_editor::send::set_http_client;
use response_viewer::set_secret::{RESEND_BUTTON_SELECTOR, set_button_selector};
use secrets::{MemorySecretStore, SecretStore};
use storage::LocalStore;
use ui::test_support::draw_window;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

/// The obviously fake secret the save writes (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";

const BASE: &str = "https://alpha.example.test";

/// The secret the fixture request references and the keychain holds nothing for.
const API_KEY: &str = "api_key";

/// The request inside the fixture's collection: it sends the secret in a header.
const COLLECTION_REQUEST_NAME: &str = "List pets";

/// One fixture workspace on disk.
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

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

/// A workspace with a `base` global, one environment (Alpha, carrying the secret
/// `api_key` with no value anywhere but the keychain) and one collection whose request
/// sends `{{api_key}}` in a header to `{{base}}/pets`.
fn fixture_workspace() -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    let workspace_id = ItemId::new();
    let globals = serde_json::to_value(vec![variable("base", Some(BASE), VariableKind::Default)])
        .expect("the globals serialize");
    let document = serde_json::json!({
        "id": workspace_id.to_string(),
        "name": "Secrets workspace",
        "format_version": "1.0",
        "variables": globals,
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the workspace file is written");

    let alpha = Environment {
        id: ItemId::new(),
        name: "Alpha".to_string(),
        variables: vec![variable(API_KEY, None, VariableKind::Secret)],
        extra: BTreeMap::new(),
    };
    save(
        &root,
        Item {
            body: ItemBody::Environment(alpha),
            dir: PathBuf::from("environments"),
            path: None,
        },
    );

    let request = HttpRequest {
        id: ItemId::new(),
        name: COLLECTION_REQUEST_NAME.to_string(),
        method: Method::Get,
        url: "{{base}}/pets".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![KeyValue {
            key: "X-Api-Key".to_string(),
            value: Some(format!("{{{{{API_KEY}}}}}")),
            description: None,
            enabled: true,
            extra: BTreeMap::new(),
        }],
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    };
    let collection = Collection {
        id: ItemId::new(),
        name: "Pets".to_string(),
        description: None,
        auth: Auth::Inherit,
        variables: Vec::new(),
        order: vec![request.id],
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    let collection_file = save(
        &root,
        Item {
            body: ItemBody::Collection(collection),
            dir: PathBuf::from("collections"),
            path: None,
        },
    );
    let collection_dir = collection_file
        .parent()
        .expect("a collection file lives in its own directory")
        .to_path_buf();
    save(
        &root,
        Item {
            body: ItemBody::Request(Request::Http(request)),
            dir: collection_dir,
            path: None,
        },
    );
    Fixture {
        _directory: directory,
        root,
    }
}

fn save(root: &Path, item: Item) -> PathBuf {
    workspace_store::save_item(root, &item).expect("the fixture item is written")
}

fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Boot the app the way the binary does, over `store`, with a `FakeTransport` client and
/// an empty memory keychain in place of the OS one, then open the fixture folder.
fn boot<'a>(
    cx: &'a mut TestAppContext,
    store: &Arc<Mutex<LocalStore>>,
    fixture: &Fixture,
) -> (Entity<Workspace>, FakeTransport, &'a mut VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(store))));
    let transport = FakeTransport::new(cx.executor());
    for _ in 0..2 {
        transport.push_response(Duration::ZERO, 200, b"ok".to_vec());
    }
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|cx| set_http_client(client, cx));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    // Replace the OS keychain the store landing installed: tests never touch it.
    let keychain: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    cx.update(|_window, cx| secret_values::install_inputs(cx, SecretInputs { store: keychain }));
    open_folder(cx, &workspace, fixture);
    (workspace, transport, cx)
}

/// Open `fixture`'s folder in the window, as File > Open Folder does.
fn open_folder(cx: &mut VisualTestContext, workspace: &Entity<Workspace>, fixture: &Fixture) {
    let opened = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    let root = fixture.root.clone();
    cx.update(|_, cx| opened.update(cx, |opener, cx| opener.open_folder(root, cx)));
    cx.run_until_parked();
    draw_window(cx);
}

/// Pick the environment named `name` the way a user does: the picker, a query, Enter.
fn select_environment(cx: &mut VisualTestContext, name: &str) {
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input(name);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
}

/// Open the fixture's collection request through quick open, as a user does.
fn open_collection_request(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
) -> Entity<RequestEditor> {
    cx.dispatch_action(ToggleQuickOpen);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input(COLLECTION_REQUEST_NAME);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
    active_editor(workspace, cx)
}

fn active_editor(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<RequestEditor> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
        })
    })
    .expect("the active tab is a request editor")
}

fn send(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.start_send(cx)));
    cx.run_until_parked();
    draw_window(cx);
}

/// Click the warning bar's "Send anyway" — `request_editor`'s own selector, which that
/// crate keeps private.
fn send_anyway(cx: &mut VisualTestContext) {
    let button = cx
        .debug_bounds("send-anyway")
        .expect("the warning bar's Send anyway button is on screen");
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);
}

/// Whether anything is on screen under this debug key (`set_secret_tests`' shape:
/// `debug_bounds` takes a `&'static str`, so the selector is leaked).
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

fn warned_names(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<Vec<String>> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor
                .unresolved_warning()
                .map(|warning| warning.names().to_vec())
        })
    })
}

fn inline_error(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor.inline_error().map(|error| error.message.clone())
        })
    })
}

/// What a send refused for an unresolved `api_key` secret shows inline.
fn unresolved_secret_message() -> String {
    TransportError::UnresolvedSecret {
        names: vec![API_KEY.to_string()],
    }
    .to_string()
}

/// Whether any request the transport received carries the fake secret, in its URL or any
/// header (`wire_scope_wiring`'s shape).
fn carries_secret(requests: &[ResolvedRequest]) -> bool {
    requests.iter().any(|request| {
        request.url.as_str().contains(FAKE_SECRET)
            || request
                .headers
                .iter()
                .any(|header| header.value.contains(FAKE_SECRET))
    })
}

#[gpui::test]
fn wire_unresolved_secret_panel_offers_resend_after_set(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let (workspace, transport, cx) = boot(cx, &store, &fixture);

    select_environment(cx, "Alpha");
    let editor = open_collection_request(cx, &workspace);
    send(&editor, cx);
    assert_eq!(
        warned_names(&editor, cx),
        Some(vec![API_KEY.to_string()]),
        "a secret with no keychain entry is unresolved before the send"
    );
    assert!(transport.requests().is_empty(), "the send is held back");

    send_anyway(cx);
    assert_eq!(
        inline_error(&editor, cx),
        Some(unresolved_secret_message()),
        "sending anyway fails as an unresolved secret"
    );
    // The panel offers Set for the missing name — and Resend only once it resolves.
    assert!(
        shown(cx, set_button_selector(API_KEY)),
        "the panel offers Set for the missing `api_key`"
    );
    assert!(
        !shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "Resend is offered on save — re-sending before anything is set would fail again"
    );

    // "Set `api_key`" lands in the variables editor on that secret's masked field.
    click(cx, set_button_selector(API_KEY));
    cx.run_until_parked();
    draw_window(cx);
    let variables = editor_of(&workspace, cx)
        .expect("the Set button opens the variables editor on the active environment");
    variables.update(cx, |variables, cx| {
        assert!(
            variables.set_secret_value(API_KEY, FAKE_SECRET, cx),
            "the editor has the `api_key` row the Set button named"
        );
    });
    cx.update(|window, cx| window.dispatch_action(SaveVariables.boxed_clone(), cx));
    cx.run_until_parked();
    draw_window(cx);

    // On save the panel offers Resend — and clicking it re-sends with the value.
    assert!(
        shown(cx, RESEND_BUTTON_SELECTOR.to_owned()),
        "after the save the panel offers Resend"
    );
    click(cx, RESEND_BUTTON_SELECTOR.to_owned());
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        carries_secret(&transport.requests()),
        "Resend's send carries the value"
    );
}
