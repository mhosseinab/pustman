//! The variable-scope wire (S43w): in the running app, every open request editor resolves
//! `{{var}}` against the opened workspace's globals, the active environment and the
//! request's collection, re-resolving when the active environment changes; secret-kind
//! variables resolve from the keychain at send time and nowhere else; the environment
//! editor opens on the active environment with no hand-installed inputs; and the pre-send
//! check treats a keychain secret as resolved (#198).
//!
//! Every test boots through the real `pustman::init_with_store` over an in-memory store
//! (shared with the test so it can read SQLite back), opens a temp workspace folder
//! through `WorkspaceOpen`, and replaces the app's OS keychain with a
//! `MemorySecretStore` — the real keychain is never touched. `Decision:` the sends go
//! through a `FakeTransport` client (installed before the store lands, so the app's
//! history recorder wires onto it): `.agents/rules/tests.md` forbids real network in GPUI
//! tests, and the transport records the resolved request exactly as a server would see
//! it. Every value is obviously fake.

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
use environment_picker::env_editor::{EnvEditor, Open as OpenEnvEditor};
use environment_picker::secret_values::{self, SecretInputs};
use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, TransportError};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use pustman_actions::quick_open::Toggle as ToggleQuickOpen;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use request_editor::send::set_http_client;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{LocalStore, LocalValue};
use ui::test_support::draw_window;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

/// The obviously fake secret the keychain holds (`.agents/rules/tests.md`).
const FAKE_SECRET: &str = "token-REDACTED-example";

const GLOBALS_BASE: &str = "https://globals.example.test";
const ALPHA_BASE: &str = "https://alpha.example.test";
const BETA_BASE: &str = "https://beta.example.test";

/// The request inside the fixture's collection: it sends the secret in a header.
const COLLECTION_REQUEST_NAME: &str = "List pets";

/// One fixture workspace on disk and the ids the tests key the keychain by.
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    workspace_id: ItemId,
    alpha: ItemId,
    beta: ItemId,
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

/// A workspace with a `base` global, two environments overriding it (Alpha also holds
/// the secret `token`, with no value anywhere but the keychain) and one collection whose
/// request sends `{{token}}` in a header to `{{base}}/{{path}}` (`path` is a collection
/// variable).
fn fixture_workspace() -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    let workspace_id = ItemId::new();
    let globals = serde_json::to_value(vec![variable(
        "base",
        Some(GLOBALS_BASE),
        VariableKind::Default,
    )])
    .expect("the globals serialize");
    let document = serde_json::json!({
        "id": workspace_id.to_string(),
        "name": "Scopes workspace",
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
        variables: vec![
            variable("base", Some(ALPHA_BASE), VariableKind::Default),
            variable("token", None, VariableKind::Secret),
        ],
        extra: BTreeMap::new(),
    };
    let beta = Environment {
        id: ItemId::new(),
        name: "Beta".to_string(),
        variables: vec![variable("base", Some(BETA_BASE), VariableKind::Default)],
        extra: BTreeMap::new(),
    };
    let alpha_id = alpha.id;
    let beta_id = beta.id;
    for environment in [alpha, beta] {
        save(
            &root,
            Item {
                body: ItemBody::Environment(environment),
                dir: PathBuf::from("environments"),
                path: None,
            },
        );
    }

    let request = HttpRequest {
        id: ItemId::new(),
        name: COLLECTION_REQUEST_NAME.to_string(),
        method: Method::Get,
        url: "{{base}}/{{path}}".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: vec![KeyValue {
            key: "X-Api-Key".to_string(),
            value: Some("{{token}}".to_string()),
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
        variables: vec![variable("path", Some("pets"), VariableKind::Default)],
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
        workspace_id,
        alpha: alpha_id,
        beta: beta_id,
    }
}

/// A second folder carrying a byte-for-byte copy of `fixture`: the same `pustman.json`
/// id and the same environment and collection ids — what anyone can commit to a repo
/// they control.
fn copy_fixture(fixture: &Fixture) -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    copy_tree(&fixture.root, &root);
    Fixture {
        _directory: directory,
        root,
        workspace_id: fixture.workspace_id,
        alpha: fixture.alpha,
        beta: fixture.beta,
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's directory is created");
    for entry in std::fs::read_dir(from).expect("the directory reads") {
        let path = entry.expect("the entry reads").path();
        let target = to.join(path.file_name().expect("an entry has a name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("the file copies");
        }
    }
}

/// The key the app keeps `fixture`'s local values, selection and keychain entries under
/// on this installation.
fn installation_key(fixture: &Fixture) -> String {
    let canonical = std::fs::canonicalize(&fixture.root).expect("the fixture folder exists");
    pustman::scope_wiring::workspace_key(&fixture.workspace_id.to_string(), &canonical)
}

/// Every workspace key a secret or local value of `fixture` is seeded under: its
/// installation key, and the bare `pustman.json` id S43w first keyed them by — so a wire
/// that ever reads by the bare id again resolves the seed, and the tests catch it.
fn seeded_keys(fixture: &Fixture) -> [String; 2] {
    [installation_key(fixture), fixture.workspace_id.to_string()]
}

/// Store the fake secret as `fixture`'s Alpha `token` (see [`seeded_keys`]).
fn seed_secret(keychain: &MemorySecretStore, fixture: &Fixture) {
    for workspace in seeded_keys(fixture) {
        let key = SecretKey::new(workspace, fixture.alpha.to_string(), "token")
            .expect("the fixture ids are valid keychain ids");
        keychain
            .set(&key, &SecretValue::new(FAKE_SECRET))
            .expect("the fixture secret stores");
    }
}

/// Store a non-secret local value of `fixture`'s scope `scope_id` (see [`seeded_keys`]).
fn seed_local_value(
    store: &Arc<Mutex<LocalStore>>,
    fixture: &Fixture,
    scope_id: &str,
    key: &str,
    value: &str,
) {
    let store = store.lock().expect("the lock is free");
    for workspace in seeded_keys(fixture) {
        store
            .local_values()
            .insert(&LocalValue {
                workspace_id: workspace,
                scope_id: scope_id.to_string(),
                key: key.to_string(),
                value: value.to_string(),
            })
            .expect("the local value stores");
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

/// The memory keychain standing in for the OS one, holding the fixture secret under
/// Alpha's `token` key when `with_secret`.
fn memory_keychain(fixture: &Fixture, with_secret: bool) -> Arc<MemorySecretStore> {
    let keychain = Arc::new(MemorySecretStore::new());
    if with_secret {
        seed_secret(&keychain, fixture);
    }
    keychain
}

/// Boot the app the way the binary does, over `store`, with a `FakeTransport` client and
/// the memory keychain in place of the OS one, then open the fixture folder.
fn boot<'a>(
    cx: &'a mut TestAppContext,
    store: &Arc<Mutex<LocalStore>>,
    fixture: &Fixture,
    keychain: Arc<MemorySecretStore>,
) -> (Entity<Workspace>, FakeTransport, &'a mut VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(store))));
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
    // Replace the OS keychain the store landing installed: tests never touch it.
    let keychain: Arc<dyn SecretStore> = keychain;
    cx.update(|_window, cx| secret_values::install_inputs(cx, SecretInputs { store: keychain }));
    open_folder(cx, &workspace, fixture);
    (workspace, transport, cx)
}

fn opener(workspace: &Entity<Workspace>, cx: &VisualTestContext) -> Entity<WorkspaceOpen> {
    cx.read(|cx| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        .expect("the window has open-workspace state")
}

/// Open `fixture`'s folder in the window, as File > Open Folder does.
fn open_folder(cx: &mut VisualTestContext, workspace: &Entity<Workspace>, fixture: &Fixture) {
    let opened = opener(workspace, cx);
    let root = fixture.root.clone();
    cx.update(|_, cx| opened.update(cx, |opener, cx| opener.open_folder(root, cx)));
    cx.run_until_parked();
    draw_window(cx);
}

/// A new request tab sending to `url`.
fn new_request(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
    url: &str,
) -> Entity<RequestEditor> {
    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    cx.simulate_input(url);
    active_editor(workspace, cx)
}

/// What a send refused for an unresolved `token` secret shows inline.
fn unresolved_token_message() -> String {
    TransportError::UnresolvedSecret {
        names: vec!["token".to_string()],
    }
    .to_string()
}

/// Click the warning bar's "Send anyway" — `request_editor`'s `SEND_ANYWAY_SELECTOR`,
/// which that crate keeps private.
fn send_anyway(cx: &mut VisualTestContext) {
    let button = cx
        .debug_bounds("send-anyway")
        .expect("the warning bar's Send anyway button is on screen");
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);
}

/// Whether any request the transport received carries the fake secret.
fn sent_secret(transport: &FakeTransport) -> bool {
    transport.requests().iter().any(|request| {
        request.url.as_str().contains(FAKE_SECRET)
            || request
                .headers
                .iter()
                .any(|header| header.value.contains(FAKE_SECRET))
    })
}

fn inline_error(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor.inline_error().map(|error| error.message.clone())
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

fn send(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.start_send(cx)));
    cx.run_until_parked();
    draw_window(cx);
}

fn last_url(transport: &FakeTransport) -> String {
    transport
        .requests()
        .last()
        .map(|request| request.url.to_string())
        .expect("the transport received a request")
}

#[gpui::test]
fn wire_variables_resolve_from_active_environment(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    cx.simulate_input("{{base}}/pets");
    let editor = active_editor(&workspace, cx);

    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{GLOBALS_BASE}/pets"),
        "with no active environment `base` resolves from the workspace globals"
    );

    select_environment(cx, "Alpha");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{ALPHA_BASE}/pets"),
        "the active environment's `base` beats the global one (narrowest wins)"
    );

    select_environment(cx, "Beta");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{BETA_BASE}/pets"),
        "switching the active environment re-resolves the open editor"
    );
}

#[gpui::test]
fn wire_secret_resolves_from_keychain_at_send(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, true);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    let editor = open_collection_request(cx, &workspace);
    send(&editor, cx);

    let request = transport
        .requests()
        .last()
        .cloned()
        .expect("the request was sent");
    assert_eq!(
        request.url.as_str(),
        format!("{ALPHA_BASE}/pets"),
        "the collection's `path` and the environment's `base` both resolve"
    );
    let sent_key = request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("x-api-key"))
        .map(|header| header.value.clone());
    assert_eq!(
        sent_key.as_deref(),
        Some(FAKE_SECRET),
        "the secret-kind variable is resolved from the keychain at send"
    );

    // No workspace file carries the value.
    let mut files = Vec::new();
    collect_files(&fixture.root, &mut files);
    for file in files {
        let bytes = std::fs::read(&file).expect("the workspace file reads");
        assert!(
            !String::from_utf8_lossy(&bytes).contains(FAKE_SECRET),
            "the secret never reaches a workspace file ({})",
            file.display()
        );
    }

    // No SQLite row carries it: history, local values, kv, tabs, layout.
    {
        let store = store.lock().expect("the lock is free");
        let today = time::OffsetDateTime::now_utc().date();
        let history = store
            .history()
            .list_by_day(pustman::WORKSPACE_ID, today)
            .expect("the history reads");
        assert!(!history.is_empty(), "the send was recorded in history");
        let dumps = [
            format!("{history:?}"),
            format!(
                "{:?}",
                store
                    .local_values()
                    .list(&fixture.workspace_id.to_string())
                    .expect("the local values read")
            ),
            format!("{:?}", store.kv().list().expect("the kv reads")),
            format!(
                "{:?}",
                store
                    .open_tabs()
                    .list(pustman::WORKSPACE_ID)
                    .expect("the tabs read")
            ),
            format!("{:?}", store.layout().list().expect("the layout reads")),
        ];
        for dump in dumps {
            assert!(
                !dump.contains(FAKE_SECRET),
                "the secret never reaches SQLite"
            );
        }
    }

    // Nor the Console.
    let entries = cx.read(|cx| {
        cx.try_global::<console::ConsoleStore>()
            .map(|console| console.entries().iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    assert!(!entries.is_empty(), "the send reached the Console");
    for entry in entries {
        for text in [&entry.url, &entry.raw_request, &entry.raw_response] {
            assert!(
                !text.contains(FAKE_SECRET),
                "the secret never reaches the Console"
            );
        }
    }
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("the directory reads") {
        let path = entry.expect("the entry reads").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

#[gpui::test]
fn wire_env_editor_opens_for_active_environment(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, _transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    cx.dispatch_action(OpenEnvEditor);
    cx.run_until_parked();
    draw_window(cx);

    let editor = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace.modal_layer().active_modal::<EnvEditor>()
            })
        })
        .expect("env_editor::Open opens the editor, with the app's inputs installed");
    let rows = cx.read(|cx| editor.read_with(cx, |editor, cx| editor.displayed_rows(cx)));
    let shown: Vec<(String, Option<String>)> = rows
        .iter()
        .map(|row| (row.key.clone(), row.shared.clone()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("base".to_string(), Some(ALPHA_BASE.to_string())),
            ("token".to_string(), None),
        ],
        "the editor opens on the active environment's variables"
    );
    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.error())),
        None,
        "the editor opens without an error (the keychain is connected)"
    );
}

#[gpui::test]
fn wire_presend_check_treats_keychain_secret_as_resolved(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, true);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    let editor = open_collection_request(cx, &workspace);
    send(&editor, cx);

    assert_eq!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.unresolved_warning().cloned())),
        None,
        "a secret-kind name with a keychain entry raises no unresolved warning (#198)"
    );
    assert_eq!(
        transport.requests().len(),
        1,
        "the send is not held back by the pre-send check"
    );
}

const LOCAL_BASE: &str = "https://local.example.test";
const EDITED_BASE: &str = "https://edited.example.test";

/// Another folder carrying the same `pustman.json` and environment ids must never resolve
/// this folder's keychain secrets or local values: the ids are committed to a shared repo,
/// so anyone can copy them into one they control.
#[gpui::test]
fn wire_secret_and_local_value_stay_with_their_folder(cx: &mut TestAppContext) {
    let folder_a = fixture_workspace();
    let folder_b = copy_fixture(&folder_a);
    let store = shared_store();
    seed_local_value(
        &store,
        &folder_a,
        &folder_a.alpha.to_string(),
        "base",
        LOCAL_BASE,
    );
    let keychain = memory_keychain(&folder_a, true);
    let (workspace, transport, cx) = boot(cx, &store, &folder_a, keychain);

    select_environment(cx, "Alpha");
    let editor = open_collection_request(cx, &workspace);
    send(&editor, cx);
    assert!(
        sent_secret(&transport),
        "folder A resolves its own keychain secret"
    );
    assert_eq!(
        last_url(&transport),
        format!("{LOCAL_BASE}/pets"),
        "folder A applies its own local value"
    );
    let sent_by_a = transport.requests().len();

    open_folder(cx, &workspace, &folder_b);
    select_environment(cx, "Alpha");
    // A new tab: quick open would bring back folder A's tab for the same request id,
    // which is bound to folder A and resolves nothing here.
    let editor = new_request(cx, &workspace, "{{base}}/{{token}}");
    send(&editor, cx);
    assert_eq!(
        warned_names(&editor, cx),
        Some(vec!["token".to_string()]),
        "folder B has no keychain entry of its own, so the pre-send check warns"
    );
    send_anyway(cx);
    assert_eq!(
        inline_error(&editor, cx),
        Some(unresolved_token_message()),
        "folder B's send fails as an unresolved secret"
    );
    assert_eq!(
        transport.requests().len(),
        sent_by_a,
        "folder B sends nothing — never an empty or borrowed value"
    );

    let editor = new_request(cx, &workspace, "{{base}}/pets");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{ALPHA_BASE}/pets"),
        "folder B resolves its shared value, not folder A's local value"
    );
    assert!(
        !transport.requests()[sent_by_a..]
            .iter()
            .any(|request| request
                .headers
                .iter()
                .any(|header| header.value.contains(FAKE_SECRET))),
        "folder A's secret never leaves from folder B"
    );
}

/// A tab left open from folder A must not resolve folder B's secrets after B opens: it
/// would send them to A's host.
#[gpui::test]
fn wire_stale_tab_from_previous_folder_never_sends_new_folders_secret(cx: &mut TestAppContext) {
    let folder_a = fixture_workspace();
    let folder_b = copy_fixture(&folder_a);
    let store = shared_store();
    let keychain = memory_keychain(&folder_b, true);
    let (workspace, transport, cx) = boot(cx, &store, &folder_a, keychain);

    select_environment(cx, "Alpha");
    let stale = open_collection_request(cx, &workspace);

    open_folder(cx, &workspace, &folder_b);
    select_environment(cx, "Alpha");
    let fresh = new_request(cx, &workspace, "{{base}}/{{token}}");
    send(&fresh, cx);
    assert!(
        sent_secret(&transport),
        "a tab opened in folder B resolves B's keychain secret"
    );
    let sent_by_b = transport.requests().len();

    send(&stale, cx);
    assert!(
        !transport.requests()[sent_by_b..]
            .iter()
            .any(|request| request
                .headers
                .iter()
                .any(|header| header.value.contains(FAKE_SECRET))),
        "the tab from folder A never sends folder B's secret"
    );
}

/// A choice made while the workspace's stored state is still being read wins over the
/// stored one when the read lands.
#[gpui::test]
fn wire_selection_made_during_state_load_survives_it(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);
    // Alpha is persisted as the workspace's active environment.
    select_environment(cx, "Alpha");

    // The store lands again (re-reading the stored state) and, before that read lands,
    // the user confirms Beta — what the picker's confirm does to the installed inputs.
    let beta = fixture.beta;
    cx.update(|_, cx| {
        cx.set_global(pustman::AppStore(Arc::clone(&store)));
        cx.global_mut::<environment_picker::SelectorInputs>().active = Some(beta);
    });
    cx.run_until_parked();

    let editor = new_request(cx, &workspace, "{{base}}/pets");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{BETA_BASE}/pets"),
        "the stale read of the stored selection does not undo the user's choice"
    );
}

#[gpui::test]
fn wire_stored_local_value_applies_only_for_the_winning_scope(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    seed_local_value(
        &store,
        &fixture,
        &fixture.alpha.to_string(),
        "base",
        LOCAL_BASE,
    );
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    let editor = new_request(cx, &workspace, "{{base}}/pets");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{GLOBALS_BASE}/pets"),
        "Alpha's local value is not applied while the global `base` wins"
    );

    select_environment(cx, "Alpha");
    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{LOCAL_BASE}/pets"),
        "Alpha's local value replaces its shared value once Alpha wins"
    );
}

/// `fixture`'s Alpha environment as the folder holds it now, with `base` set to `value`.
fn alpha_with_base(fixture: &Fixture, value: &str) -> Item {
    let snapshot = workspace_store::open(&fixture.root).expect("the fixture folder opens");
    let mut item = snapshot
        .items
        .get(&fixture.alpha)
        .cloned()
        .expect("the folder holds Alpha");
    if let ItemBody::Environment(environment) = &mut item.body {
        for variable in &mut environment.variables {
            if variable.key == "base" {
                variable.value = Some(value.to_string());
            }
        }
    }
    item
}

#[gpui::test]
fn wire_env_editor_save_re_resolves_open_editors(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    let editor = new_request(cx, &workspace, "{{base}}/pets");
    send(&editor, cx);
    assert_eq!(last_url(&transport), format!("{ALPHA_BASE}/pets"));

    // What the environment editor does to the installed inputs after a save.
    let target =
        environment_picker::env_editor::EditTarget::new(alpha_with_base(&fixture, EDITED_BASE))
            .expect("Alpha is an editable scope");
    cx.update(|_, cx| {
        cx.global_mut::<environment_picker::env_editor::EnvEditorInputs>()
            .target = target;
    });
    cx.run_until_parked();

    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{EDITED_BASE}/pets"),
        "the saved value reaches the open editor"
    );
}

#[gpui::test]
fn wire_snapshot_change_on_disk_re_resolves_open_editors(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    let editor = new_request(cx, &workspace, "{{base}}/pets");
    send(&editor, cx);
    assert_eq!(last_url(&transport), format!("{ALPHA_BASE}/pets"));

    // The file is edited on disk and the watcher hands the new snapshot over.
    save(&fixture.root, alpha_with_base(&fixture, EDITED_BASE));
    let snapshot = workspace_store::open(&fixture.root).expect("the fixture folder opens");
    let opened = opener(&workspace, cx);
    cx.update(|_, cx| opened.update(cx, |opener, cx| opener.set_snapshot(snapshot, cx)));
    cx.run_until_parked();

    send(&editor, cx);
    assert_eq!(
        last_url(&transport),
        format!("{EDITED_BASE}/pets"),
        "the edited file reaches the open editor"
    );
}

#[gpui::test]
fn wire_secret_without_keychain_entry_warns_and_fails_unresolved(cx: &mut TestAppContext) {
    let fixture = fixture_workspace();
    let store = shared_store();
    let keychain = memory_keychain(&fixture, false);
    let (workspace, transport, cx) = boot(cx, &store, &fixture, keychain);

    select_environment(cx, "Alpha");
    let editor = open_collection_request(cx, &workspace);
    send(&editor, cx);
    assert_eq!(
        warned_names(&editor, cx),
        Some(vec!["token".to_string()]),
        "a secret with nothing in the keychain is unresolved before the send"
    );
    assert!(transport.requests().is_empty(), "the send is held back");

    send_anyway(cx);
    assert_eq!(
        inline_error(&editor, cx),
        Some(unresolved_token_message()),
        "sending anyway fails as an unresolved secret"
    );
    assert!(
        transport.requests().is_empty(),
        "nothing reaches the wire — never an empty value"
    );
}
