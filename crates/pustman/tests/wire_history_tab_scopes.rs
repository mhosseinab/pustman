//! A request tab opened from History is an unsaved request of the window it opens in: it
//! resolves `{{var}}` against that window's active environment and globals, like a New
//! Request tab does — and the keychain only for a request the open workspace holds at
//! the URL and settings the entry recorded (the S43w rule). Once bound, a tab keeps its
//! workspace: another workspace opening later never lends it its scopes or secrets.
//!
//! Boots through the real `pustman::init_with_store` over an in-memory store, opens temp
//! workspace folders through `WorkspaceOpen`, replaces the OS keychain with a
//! `MemorySecretStore` and sends through a `FakeTransport`. Every value is obviously fake.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{
    Auth, Body, Collection, Environment, HttpRequest, ItemId, KeyValue, Method, Request, Variable,
    VariableKind,
};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, ResolvedRequest};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use request_editor::RequestEditor;
use request_editor::send::set_http_client;
use request_editor::variable_line::EntryState;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{HistoryEntry, LocalStore};
use ui::test_support::draw_window;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

/// The obviously fake secret the keychain holds (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";

const DEV_BASE: &str = "https://api.dev.example.test";
const OTHER_BASE: &str = "https://api.other.example.test";

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    workspace_id: ItemId,
    development: ItemId,
    /// The request the fixture's collection holds: `{{base_url}}/saved`, sending
    /// `{{token}}` in a header.
    saved_request: HttpRequest,
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

fn save(root: &Path, item: Item) -> PathBuf {
    workspace_store::save_item(root, &item).expect("the fixture item is written")
}

/// A workspace whose "Development" environment defines `base_url` as `base` and holds
/// the secret `token` (no value anywhere but the keychain), with one collection request.
fn fixture(base: &str) -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    let workspace_id = ItemId::new();
    let document = serde_json::json!({
        "id": workspace_id.to_string(),
        "name": "History scopes workspace",
        "format_version": "1.0",
    });
    std::fs::write(
        root.join("pustman.json"),
        serde_json::to_string_pretty(&document).expect("the fixture serializes"),
    )
    .expect("the workspace file is written");
    let development = Environment {
        id: ItemId::new(),
        name: "Development".to_string(),
        variables: vec![
            variable("base_url", Some(base), VariableKind::Default),
            variable("token", None, VariableKind::Secret),
        ],
        extra: BTreeMap::new(),
    };
    let development_id = development.id;
    save(
        &root,
        Item {
            body: ItemBody::Environment(development),
            dir: PathBuf::from("environments"),
            path: None,
        },
    );
    let saved_request = HttpRequest {
        headers: vec![KeyValue {
            key: "X-Api-Key".to_string(),
            value: Some("{{token}}".to_string()),
            description: None,
            enabled: true,
            extra: BTreeMap::new(),
        }],
        name: "Saved".to_string(),
        ..http_request("{{base_url}}/saved")
    };
    let collection = Collection {
        id: ItemId::new(),
        name: "Saved requests".to_string(),
        description: None,
        auth: Auth::Inherit,
        variables: Vec::new(),
        order: vec![saved_request.id],
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
            body: ItemBody::Request(Request::Http(saved_request.clone())),
            dir: collection_dir,
            path: None,
        },
    );
    Fixture {
        _directory: directory,
        root,
        workspace_id,
        development: development_id,
        saved_request,
    }
}

/// An ad-hoc GET to `url` with nothing else set.
fn http_request(url: &str) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "Untitled".to_string(),
        method: Method::Get,
        url: url.to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// Store the fake secret as `fixture`'s Development `token` on this installation.
fn seed_secret(keychain: &MemorySecretStore, fixture: &Fixture) {
    let canonical = std::fs::canonicalize(&fixture.root).expect("the fixture folder exists");
    let workspace_key =
        pustman::scope_wiring::workspace_key(&fixture.workspace_id.to_string(), &canonical);
    let key = SecretKey::new(workspace_key, fixture.development.to_string(), "token")
        .expect("the fixture ids are valid keychain ids");
    keychain
        .set(&key, &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret stores");
}

/// A recorded send of `request`, as History holds it: the pre-resolution request.
fn history_entry(request: &HttpRequest) -> HistoryEntry {
    HistoryEntry {
        id: None,
        workspace_id: pustman::WORKSPACE_ID.to_string(),
        created_at: time::OffsetDateTime::now_utc(),
        protocol: "http".to_string(),
        method: "GET".to_string(),
        target: request.url.clone(),
        status: None,
        duration_ms: None,
        request_json: serde_json::to_value(request).expect("the request serializes"),
        response_summary_json: None,
        response_body_prefix: None,
    }
}

/// Boot the app the way the binary does, with a `FakeTransport` client and `keychain`
/// in place of the OS one, then open `fixture`'s folder.
fn boot<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
    keychain: Arc<MemorySecretStore>,
) -> (Entity<Workspace>, FakeTransport, &'a mut VisualTestContext) {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(store)));
    let transport = FakeTransport::new(cx.executor());
    for _ in 0..4 {
        transport.push_response(Duration::ZERO, 200, b"ok".to_vec());
    }
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|cx| set_http_client(client, cx));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    cx.update(|_window, cx| {
        environment_picker::secret_values::install_inputs(
            cx,
            environment_picker::secret_values::SecretInputs { store: keychain },
        )
    });
    open_folder(cx, &workspace, fixture);
    (workspace, transport, cx)
}

fn open_folder(cx: &mut VisualTestContext, workspace: &Entity<Workspace>, fixture: &Fixture) {
    let opener = cx
        .read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state");
    let root = fixture.root.clone();
    cx.update(|_, cx| opener.update(cx, |opener, cx| opener.open_folder(root, cx)));
    cx.run_until_parked();
    draw_window(cx);
}

fn select_development(cx: &mut VisualTestContext) {
    cx.dispatch_action(SelectEnvironment);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input("Development");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
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

/// Open `entry` the way clicking it in the History rail does; the tab it opens.
fn open_history(
    entry: &HistoryEntry,
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<RequestEditor> {
    cx.update(|window, cx| {
        history_panel::open_entry::open_entry(entry, workspace, window, cx);
    });
    cx.run_until_parked();
    draw_window(cx);
    active_editor(workspace, cx)
}

fn state_of(editor: &Entity<RequestEditor>, cx: &VisualTestContext, name: &str) -> EntryState {
    cx.read(|cx| {
        editor
            .read(cx)
            .variable_line()
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.state.clone())
            .unwrap_or_else(|| panic!("{name} is on the variable line"))
    })
}

fn send(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.start_send(cx)));
    cx.run_until_parked();
    draw_window(cx);
}

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
fn wire_history_tab_resolves_the_windows_active_environment(cx: &mut TestAppContext) {
    let folder = fixture(DEV_BASE);
    let keychain = Arc::new(MemorySecretStore::new());
    let (workspace, transport, cx) = boot(cx, &folder, keychain);
    select_development(cx);

    let editor = open_history(
        &history_entry(&http_request("{{base_url}}/x")),
        &workspace,
        cx,
    );

    match state_of(&editor, cx, "base_url") {
        EntryState::Resolved { source, .. } => assert_eq!(source, "Source: Development"),
        other => panic!("base_url resolves from the active environment, got {other:?}"),
    }
    send(&editor, cx);
    assert_eq!(
        transport
            .requests()
            .last()
            .map(|request| request.url.to_string()),
        Some(format!("{DEV_BASE}/x")),
        "the history tab sends to the active environment's base_url"
    );
}

#[gpui::test]
fn wire_history_tab_of_a_request_the_workspace_holds_resolves_its_keychain_secret(
    cx: &mut TestAppContext,
) {
    let folder = fixture(DEV_BASE);
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder);
    let (workspace, transport, cx) = boot(cx, &folder, keychain);
    select_development(cx);

    let editor = open_history(&history_entry(&folder.saved_request), &workspace, cx);
    send(&editor, cx);

    assert!(
        carries_secret(&transport.requests()),
        "a history tab of a request this workspace holds resolves its keychain secret"
    );
    assert_eq!(
        transport
            .requests()
            .last()
            .map(|request| request.url.to_string()),
        Some(format!("{DEV_BASE}/saved")),
    );
}

#[gpui::test]
fn wire_tab_bound_to_one_workspace_never_takes_another_workspaces_scopes(cx: &mut TestAppContext) {
    let folder_a = fixture(DEV_BASE);
    let folder_b = fixture(OTHER_BASE);
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder_b);
    let (workspace, transport, cx) = boot(cx, &folder_a, Arc::clone(&keychain));
    select_development(cx);

    // Bound to folder A: a history tab of A's own request, opened while A is open.
    let editor = open_history(&history_entry(&folder_a.saved_request), &workspace, cx);
    assert!(
        matches!(
            state_of(&editor, cx, "base_url"),
            EntryState::Resolved { .. }
        ),
        "the history tab is bound to folder A"
    );

    open_folder(cx, &workspace, &folder_b);
    select_development(cx);
    send(&editor, cx);

    assert!(
        !carries_secret(&transport.requests()),
        "a tab bound to folder A never sends folder B's secret"
    );
    assert!(
        !transport
            .requests()
            .iter()
            .any(|request| request.url.as_str().starts_with(OTHER_BASE)),
        "a tab bound to folder A never resolves folder B's environment"
    );
    assert!(
        !matches!(
            state_of(&editor, cx, "base_url"),
            EntryState::Resolved { .. }
        ),
        "folder A's tab resolves nothing from folder B"
    );
}
