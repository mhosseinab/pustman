//! History tabs and the folder they were recorded in (issue #334): every new history row
//! keeps the installation key of the folder its tab was bound to, and a tab reopened from
//! it resolves the open folder's active environment and globals only when that is the
//! same installation — the keychain and the collection only when, in addition, the
//! folder holds the recorded request at the recorded URL and settings. An entry from
//! another folder, from a crafted copy of the folder, or from before the key was recorded
//! stays unbound (`pustman::scope_wiring`'s module docs).
//!
//! Every test boots through the real `pustman::init_with_store` over an in-memory store,
//! opens temp workspace folders through `WorkspaceOpen`, replaces the OS keychain with a
//! `MemorySecretStore`, sends through a `FakeTransport` (so the app's history recorder
//! writes the rows) and reopens entries the way a user does: `history::Focus`, Down,
//! Enter. Every value is obviously fake.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{
    Auth, Body, Collection, Environment, HttpRequest, ItemId, Method, Request, Variable,
    VariableKind,
};
use environment_picker::secret_values::{self, SecretInputs};
use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient, ResolvedRequest};
use pustman::{StartupProblems, StoreLocation};
use pustman_actions::environment::SelectEnvironment;
use pustman_actions::history::Focus as FocusHistory;
use pustman_actions::quick_open::Toggle as ToggleQuickOpen;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use request_editor::send::set_http_client;
use request_editor::variable_line::EntryState;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use storage::{HistoryEntry, LocalStore};
use ui::test_support::draw_window;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

/// The obviously fake secrets the keychain holds (`.agents/rules/tests.md`): one per
/// folder, so a send shows whose it resolved.
const FOLDER_A_SECRET: &str = "token-REDACTED-example-a";
const FOLDER_B_SECRET: &str = "token-REDACTED-example-b";

const A_BASE: &str = "https://folder-a.example.test";
const B_BASE: &str = "https://folder-b.example.test";

/// The request inside each fixture's collection.
const SAVED_REQUEST_NAME: &str = "Saved pets";

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    workspace_id: ItemId,
    development: ItemId,
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
/// the secret `token` (no value anywhere but the keychain), with one collection request
/// sending `{{token}}` in its path, `{{base_url}}/saved/{{token}}` — a header would not
/// do: history masks an `X-Api-Key`'s value, so the reopened tab would carry no
/// `{{token}}` to resolve.
fn fixture(base: &str) -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    let workspace_id = ItemId::new();
    let document = serde_json::json!({
        "id": workspace_id.to_string(),
        "name": "History origin workspace",
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
        id: ItemId::new(),
        name: SAVED_REQUEST_NAME.to_string(),
        method: Method::Get,
        url: "{{base_url}}/saved/{{token}}".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
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
            body: ItemBody::Request(Request::Http(saved_request)),
            dir: collection_dir,
            path: None,
        },
    );
    Fixture {
        _directory: directory,
        root,
        workspace_id,
        development: development_id,
    }
}

/// A second folder holding a byte-for-byte copy of `fixture` — the same `pustman.json`
/// id, environment, collection and request ids, URLs and settings — but with
/// `base_url` aimed at `base`: what anyone can commit to a repo they control.
fn crafted_copy(fixture: &Fixture, base: &str) -> Fixture {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let root = directory.path().to_path_buf();
    copy_tree(&fixture.root, &root);
    let environment_file = find_file(&root.join("environments"));
    let text = std::fs::read_to_string(&environment_file).expect("the environment reads");
    std::fs::write(&environment_file, text.replace(A_BASE, base))
        .expect("the environment is rewritten");
    Fixture {
        _directory: directory,
        root,
        workspace_id: fixture.workspace_id,
        development: fixture.development,
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

/// The one file under `directory` (the fixture's environments hold one).
fn find_file(directory: &Path) -> PathBuf {
    let path = std::fs::read_dir(directory)
        .expect("the directory reads")
        .next()
        .unwrap_or_else(|| panic!("{} holds no file", directory.display()))
        .expect("the entry reads")
        .path();
    if path.is_dir() {
        find_file(&path)
    } else {
        path
    }
}

/// Store `secret` as `fixture`'s Development `token` on this installation.
fn seed_secret(keychain: &MemorySecretStore, fixture: &Fixture, secret: &str) {
    let canonical = std::fs::canonicalize(&fixture.root).expect("the fixture folder exists");
    let workspace_key =
        pustman::scope_wiring::workspace_key(&fixture.workspace_id.to_string(), &canonical);
    let key = SecretKey::new(workspace_key, fixture.development.to_string(), "token")
        .expect("the fixture ids are valid keychain ids");
    keychain
        .set(&key, &SecretValue::new(secret))
        .expect("the fixture secret stores");
}

fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// Boot the app the way the binary does, over `store`, with a `FakeTransport` client and
/// `keychain` in place of the OS one, then open `fixture`'s folder.
fn boot<'a>(
    cx: &'a mut TestAppContext,
    store: &Arc<Mutex<LocalStore>>,
    fixture: &Fixture,
    keychain: Arc<MemorySecretStore>,
) -> (Entity<Workspace>, FakeTransport, &'a mut VisualTestContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(store))));
    let transport = FakeTransport::new(cx.executor());
    for _ in 0..8 {
        transport.push_response(Duration::ZERO, 200, b"ok".to_vec());
    }
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(|cx| set_http_client(client, cx));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    cx.update(|_window, cx| secret_values::install_inputs(cx, SecretInputs { store: keychain }));
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

/// A New Request tab sending to `url`.
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

/// Open the fixture's collection request through quick open, as a user does.
fn open_saved_request(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
) -> Entity<RequestEditor> {
    cx.dispatch_action(ToggleQuickOpen);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_input(SAVED_REQUEST_NAME);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);
    active_editor(workspace, cx)
}

/// Reopen the newest history entry the way a user does — focus the History panel, Down
/// onto the first entry, Enter — and return the tab it opens.
fn reopen_newest_entry(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
) -> Entity<RequestEditor> {
    let before = active_editor_id(workspace, cx);
    cx.dispatch_action(FocusHistory);
    cx.run_until_parked();
    draw_window(cx);
    cx.simulate_keystrokes("down");
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    draw_window(cx);
    cx.run_until_parked();
    draw_window(cx);
    let reopened = active_editor(workspace, cx);
    assert_ne!(
        Some(reopened.entity_id()),
        before,
        "the history entry opens in a tab of its own"
    );
    reopened
}

fn active_editor_id(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Option<gpui::EntityId> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .center_pane()
                .read(cx)
                .active_item()
                .map(|item| item.to_any_view().entity_id())
        })
    })
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

/// Whether `name` shows as resolved. A tab bound to no folder may have no line at all:
/// nothing ever gave it scopes to compute one against.
fn is_resolved(editor: &Entity<RequestEditor>, cx: &VisualTestContext, name: &str) -> bool {
    cx.read(|cx| {
        editor
            .read(cx)
            .variable_line()
            .iter()
            .any(|entry| entry.name == name && matches!(entry.state, EntryState::Resolved { .. }))
    })
}

fn send(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.start_send(cx)));
    cx.run_until_parked();
    draw_window(cx);
}

/// Click the warning bar's "Send anyway" (`request_editor`'s private selector).
fn send_anyway(cx: &mut VisualTestContext) {
    let button = cx
        .debug_bounds("send-anyway")
        .expect("the warning bar's Send anyway button is on screen");
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    draw_window(cx);
}

fn warned_names(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor
                .unresolved_warning()
                .map(|warning| warning.names().to_vec())
                .unwrap_or_default()
        })
    })
}

/// Whether any of `requests` carries `secret`, in its URL or any header.
fn carries(requests: &[ResolvedRequest], secret: &str) -> bool {
    requests.iter().any(|request| {
        request.url.as_str().contains(secret)
            || request
                .headers
                .iter()
                .any(|header| header.value.contains(secret))
    })
}

fn sent_to(requests: &[ResolvedRequest], base: &str) -> bool {
    requests
        .iter()
        .any(|request| request.url.as_str().starts_with(base))
}

/// What the reopened tab must show and do when it resolves nothing from the open
/// folder: no environment source on its variable line, and a send that is either held
/// back (an unresolved `{{base_url}}` host is no URL at all) or warns and — with Send
/// anyway — goes out with the literal `{{base_url}}`, never the open folder's host or
/// any keychain secret.
fn assert_unbound(
    reopened: &Entity<RequestEditor>,
    transport: &FakeTransport,
    sent_before: usize,
    cx: &mut VisualTestContext,
) {
    assert!(
        !is_resolved(reopened, cx, "base_url"),
        "the reopened entry resolves nothing from the open folder's environment"
    );
    send(reopened, cx);
    let warned = warned_names(reopened, cx);
    if !warned.is_empty() {
        assert!(
            warned.iter().any(|name| name == "base_url")
                && warned.iter().any(|name| name == "token"),
            "the pre-send check names every unresolved name, got {warned:?}"
        );
        assert_eq!(
            transport.requests().len(),
            sent_before,
            "the send is held back while the bar shows"
        );
        send_anyway(cx);
    }
    let after = transport.requests();
    let after = after.get(sent_before..).unwrap_or_default();
    assert!(
        !carries(after, FOLDER_B_SECRET) && !carries(after, FOLDER_A_SECRET),
        "the reopened entry never sends a keychain secret"
    );
    assert!(
        !sent_to(after, B_BASE)
            && !after
                .iter()
                .any(|request| request.url.as_str().contains("folder-b.example.test")),
        "the reopened entry never sends to the open folder's environment host"
    );
    assert!(
        after.iter().all(|request| {
            let url = request.url.as_str();
            url.contains("{{base_url}}") || url.contains("%7B%7Bbase_url%7D%7D")
        }),
        "whatever goes out carries the literal {{{{base_url}}}}"
    );
    assert!(
        warned.is_empty() || !after.is_empty(),
        "Send anyway sends the literal request"
    );
}

#[gpui::test]
fn wire_history_entry_recorded_in_this_folder_resolves_its_active_environment(
    cx: &mut TestAppContext,
) {
    let folder = fixture(A_BASE);
    let store = shared_store();
    let keychain = Arc::new(MemorySecretStore::new());
    let (workspace, transport, cx) = boot(cx, &store, &folder, keychain);
    select_development(cx);

    let editor = new_request(cx, &workspace, "{{base_url}}/recorded");
    send(&editor, cx);
    assert_eq!(transport.requests().len(), 1, "the original send went out");

    let reopened = reopen_newest_entry(cx, &workspace);
    match state_of(&reopened, cx, "base_url") {
        EntryState::Resolved { source, .. } => assert_eq!(source, "Source: Development"),
        other => panic!("base_url resolves from the active environment, got {other:?}"),
    }
    send(&reopened, cx);
    assert_eq!(
        transport
            .requests()
            .last()
            .map(|request| request.url.to_string()),
        Some(format!("{A_BASE}/recorded")),
        "the reopened entry sends with the environment's value"
    );
}

#[gpui::test]
fn wire_history_entry_of_this_folders_saved_request_resolves_its_keychain_secret(
    cx: &mut TestAppContext,
) {
    let folder = fixture(A_BASE);
    let store = shared_store();
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder, FOLDER_A_SECRET);
    let (workspace, transport, cx) = boot(cx, &store, &folder, keychain);
    select_development(cx);

    let saved = open_saved_request(cx, &workspace);
    send(&saved, cx);
    assert!(carries(&transport.requests(), FOLDER_A_SECRET));
    let sent_before = transport.requests().len();

    let reopened = reopen_newest_entry(cx, &workspace);
    send(&reopened, cx);
    let after = transport.requests();
    let after = after.get(sent_before..).unwrap_or_default();
    assert!(
        carries(after, FOLDER_A_SECRET),
        "an entry of a request this folder holds, recorded here, resolves its keychain secret"
    );
    assert!(sent_to(after, A_BASE));
}

#[gpui::test]
fn wire_history_entry_recorded_in_another_folder_resolves_nothing_from_the_open_one(
    cx: &mut TestAppContext,
) {
    let folder_a = fixture(A_BASE);
    let folder_b = fixture(B_BASE);
    let store = shared_store();
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder_a, FOLDER_A_SECRET);
    seed_secret(&keychain, &folder_b, FOLDER_B_SECRET);
    let (workspace, transport, cx) = boot(cx, &store, &folder_a, keychain);
    select_development(cx);
    let editor = new_request(
        cx,
        &workspace,
        &format!("{A_BASE}/recorded?key={{{{token}}}}&to={{{{base_url}}}}"),
    );
    send(&editor, cx);
    assert!(
        carries(&transport.requests(), FOLDER_A_SECRET),
        "folder A resolves its own secret"
    );

    open_folder(cx, &workspace, &folder_b);
    select_development(cx);
    let sent_before = transport.requests().len();
    let reopened = reopen_newest_entry(cx, &workspace);
    assert_unbound(&reopened, &transport, sent_before, cx);
}

/// The case a request-id check cannot catch: folder B is a crafted copy of A — the same
/// `pustman.json` id and the same request at the same URL and settings — aimed at its
/// own host, with its own secret.
#[gpui::test]
fn wire_history_entry_never_binds_to_a_crafted_folder_holding_its_request(cx: &mut TestAppContext) {
    let folder_a = fixture(A_BASE);
    let folder_b = crafted_copy(&folder_a, B_BASE);
    let store = shared_store();
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder_a, FOLDER_A_SECRET);
    seed_secret(&keychain, &folder_b, FOLDER_B_SECRET);
    let (workspace, transport, cx) = boot(cx, &store, &folder_a, keychain);
    select_development(cx);
    let saved = open_saved_request(cx, &workspace);
    send(&saved, cx);
    assert!(carries(&transport.requests(), FOLDER_A_SECRET));

    open_folder(cx, &workspace, &folder_b);
    select_development(cx);
    let sent_before = transport.requests().len();
    let reopened = reopen_newest_entry(cx, &workspace);
    assert_unbound(&reopened, &transport, sent_before, cx);
}

/// A row recorded before the installation key was (migration 0002) names no folder: it
/// stays unbound, exactly as before #334.
#[gpui::test]
fn wire_history_entry_with_no_recorded_folder_stays_unbound(cx: &mut TestAppContext) {
    let folder = fixture(A_BASE);
    let store = shared_store();
    let request = HttpRequest {
        id: ItemId::new(),
        name: "Old entry".to_string(),
        method: Method::Get,
        url: format!("{A_BASE}/old?key={{{{token}}}}&to={{{{base_url}}}}"),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    };
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&HistoryEntry {
            id: None,
            workspace_id: pustman::WORKSPACE_ID.to_string(),
            created_at: time::OffsetDateTime::now_utc(),
            protocol: "http".to_string(),
            method: "GET".to_string(),
            target: request.url.clone(),
            status: None,
            duration_ms: None,
            request_json: serde_json::to_value(&request).expect("the request serializes"),
            response_summary_json: None,
            response_body_prefix: None,
        })
        .expect("the old row stores");
    let keychain = Arc::new(MemorySecretStore::new());
    seed_secret(&keychain, &folder, FOLDER_A_SECRET);
    let (workspace, transport, cx) = boot(cx, &store, &folder, keychain);
    select_development(cx);

    let reopened = reopen_newest_entry(cx, &workspace);
    assert_unbound(&reopened, &transport, 0, cx);
}
