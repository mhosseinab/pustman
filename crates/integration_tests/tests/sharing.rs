//! The sharing AC (PRD §6.3) end to end: two clones of one Git repository.
//!
//! Teammate A saves a request that uses `{{api_key}}` and an environment whose `api_key`
//! is a `Secret`, sets the value in the keychain, commits and pushes. Teammate B — the
//! same `pustman.json` id, a different folder — opens the workspace snapshot, watches it,
//! and pulls. The test proves the whole chain: within 1 s the request appears in B's tree
//! as an [`FsChange::ItemAdded`]; B's environment shows `api_key` as a secret with no
//! value; preparing the send in B fails with `UnresolvedSecret { names: ["api_key"] }`;
//! and A's commit carries no secret value.
//!
//! # Why one keychain, two installation keys (S43w)
//!
//! Keychain entries are keyed per installation: the `pustman.json` id bound to a digest
//! of the canonical folder it was opened from (`pustman::scope_wiring::workspace_key`).
//! Both clones commit the same `pustman.json`, so a bare-id key would let clone B read
//! clone A's secret from the one OS keychain the two teammates' machines do *not* share —
//! and one machine with two clones does. The test therefore models the sharpest case:
//! **one** [`MemorySecretStore`] stands for one OS keychain, A's entry holds the value
//! under A's installation key, and B — whose key differs — must not resolve it. The
//! positive control (A's own key resolves against the same store) proves the store is not
//! empty, so the only thing that can make B fail is the keying.
//!
//! The key derivation is the app's own function, never a copy: a test that re-derived it
//! would keep passing while the app changed underneath it.
//!
//! # Fixtures and environment
//!
//! The secret value is obviously fake (`.agents/rules/tests.md`): `S3cr3t-Api-Key-456` is
//! leetspeak, not a credential. Only `MemorySecretStore` is used — never the OS keychain —
//! and every repository is a local bare repo in temp dirs; nothing touches the network.
//! The value never reaches a `Debug`/`Display` line: assertions name the sink, never the
//! value. The wait for the watcher is a real-time bounded wait of 1 s (the step's
//! "bounded wait" alternative to a GPUI test clock), like `workspace_store`'s own watcher
//! tests: the watcher debounces on real time in a background thread, and preparing a send
//! (`http_engine::prepare`) needs no GPUI context at all.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helper fns below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, Method, Request, Variable, VariableKind,
    Workspace,
};
use http_engine::{PipelineError, TransportError, prepare};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use smol::process::Command;
use variables::{DynamicValues, ScopeKind, ScopeStack};
use workspace_store::{FORMAT_VERSION, FsChange, Item, ItemBody, WorkspaceSnapshot};

/// The secret's local value. Obviously fake: leetspeak, and no scanner's pattern
/// (`.agents/rules/tests.md`).
const SECRET: &str = "S3cr3t-Api-Key-456";
/// The `{{name}}` the fixture request references the secret by.
const SECRET_KEY: &str = "api_key";
/// How long B's tree may take to show the pulled request (PRD §6.3 AC: within 1 s).
const WAIT: Duration = Duration::from_secs(1);

/// The sharing AC, one scenario per the step: `sharing_ac_two_clones`.
#[test]
fn sharing_ac_two_clones() {
    if !git_installed() {
        eprintln!(
            "skipping sharing_ac_two_clones: `git` is not installed, so two clones of a \
             repository cannot be made"
        );
        return;
    }

    let scratch = tempfile::tempdir().expect("the scratch directory opens");
    run_git(scratch.path(), &["init", "--bare", "origin.git"]);

    // Clone A first, so the shared history exists before B's clone: the workspace itself
    // lands in the first commit, the request and environment in the second one the AC
    // describes.
    run_git(scratch.path(), &["clone", "origin.git", "team-a"]);
    let root_a = scratch.path().join("team-a");
    configure_identity(&root_a, "Teammate A", "a@example.test");

    let workspace_id = ItemId::new();
    let environment_id = ItemId::new();
    let request_id = ItemId::new();
    save(&root_a, workspace_item(workspace_id));
    run_git(&root_a, &["add", "-A"]);
    run_git(&root_a, &["commit", "-m", "Add the shared workspace"]);
    run_git(&root_a, &["push", "-u", "origin", "HEAD"]);

    // Teammate B's clone, as it stands before A shares anything.
    run_git(scratch.path(), &["clone", "origin.git", "team-b"]);
    let root_b = scratch.path().join("team-b");
    configure_identity(&root_b, "Teammate B", "b@example.test");

    // A: the request using `{{api_key}}` and the environment whose `api_key` is a secret
    // with no file-carried value — the value goes to the keychain, never to Git.
    save(&root_a, environment_item(environment_id));
    save(&root_a, request_item(request_id));
    run_git(&root_a, &["add", "-A"]);
    run_git(
        &root_a,
        &["commit", "-m", "Share the request and environment"],
    );
    run_git(&root_a, &["push"]);

    // B: Pustman is open on the snapshot and watching before the pull lands.
    let before = workspace_store::open(&root_b).expect("B's clone opens as a workspace");
    assert!(
        !before.items.contains_key(&request_id),
        "B must not have the shared request before the pull"
    );
    let (_watcher, changes) = workspace_store::watch(&root_b).expect("B's workspace watches");

    let deadline = Instant::now() + WAIT;
    run_git(&root_b, &["pull"]);

    // Within 1 s of the pull the request appears in B's tree (REQ-COLLAB-01).
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("no ItemAdded for the shared request within 1 s; events seen: {seen:?}");
        }
        match changes.recv_timeout(remaining) {
            Ok(event) => {
                if matches!(&event, FsChange::ItemAdded { id, .. } if *id == request_id) {
                    break;
                }
                seen.push(event);
            }
            Err(RecvTimeoutError::Timeout) => {
                panic!("no ItemAdded for the shared request within 1 s; events seen: {seen:?}");
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!(
                    "the watcher stopped before reporting the shared request; events seen: {seen:?}"
                );
            }
        }
    }

    // B's environment shows `api_key` as a secret, with no value.
    let after = workspace_store::open(&root_b).expect("B's clone reopens as a workspace");
    assert!(
        after.problems.is_empty(),
        "the pulled workspace reads cleanly: {:?}",
        after.problems
    );
    let environment = environment_of(&after, environment_id);
    let variable = environment
        .variables
        .iter()
        .find(|variable| variable.key == SECRET_KEY)
        .expect("B's environment carries the api_key variable");
    assert_eq!(variable.kind, VariableKind::Secret, "api_key is a secret");
    assert!(
        variable.value.is_none(),
        "B's copy of the environment carries no secret value"
    );
    let request = request_of(&after, request_id);

    // One keychain, two installations: A stores the value under its own installation key.
    let keychain = MemorySecretStore::new();
    let installation_a = installation_key(&root_a, workspace_id);
    let installation_b = installation_key(&root_b, workspace_id);
    let key_a = SecretKey::new(
        installation_a.as_str(),
        environment_id.to_string(),
        SECRET_KEY,
    )
    .expect("the fixture ids are valid keychain ids");
    keychain
        .set(&key_a, &SecretValue::new(SECRET))
        .expect("A's secret stores under A's installation key");

    let scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, environment.variables.clone());
        scopes
    };

    // The positive control: A's own key resolves against this very store, so a failure
    // below can only be the keying, not an empty keychain.
    let lookup_a = secret_keys(installation_a, environment_id.to_string());
    let prepared_a = prepare(
        &request,
        &[],
        &scopes,
        &keychain,
        &lookup_a,
        &mut DynamicValues::new(),
    );
    assert!(
        prepared_a.is_ok(),
        "clone A's own installation key resolves the secret; the keychain is not empty: {:?}",
        prepared_a.err()
    );

    // The AC's send in B: preparing it fails as unresolved secret, naming api_key.
    let lookup_b = secret_keys(installation_b.clone(), environment_id.to_string());
    let prepared_b = prepare(
        &request,
        &[],
        &scopes,
        &keychain,
        &lookup_b,
        &mut DynamicValues::new(),
    );
    match prepared_b {
        Err(PipelineError::Transport(TransportError::UnresolvedSecret { names })) => {
            assert_eq!(names, [SECRET_KEY], "B's send names exactly the secret");
        }
        other => {
            panic!("preparing the send in clone B must fail with UnresolvedSecret, got {other:?}")
        }
    }

    // The keying itself: the two clones never name the same keychain entry, however
    // identical their committed `pustman.json` is.
    let key_b = SecretKey::new(
        installation_b.as_str(),
        environment_id.to_string(),
        SECRET_KEY,
    )
    .expect("the fixture ids are valid keychain ids");
    assert_ne!(
        key_a, key_b,
        "clone B's keychain key must differ from clone A's"
    );

    // `git show` of A's commit — the diff — carries no secret value.
    let commit = run_git(&root_a, &["show", "HEAD"]);
    assert!(
        !contains(commit.as_bytes(), SECRET.as_bytes()),
        "A's commit leaked the secret value"
    );
}

/// The installation key the app keeps one clone's local values and keychain entries under
/// (S43w): the `pustman.json` id bound to the canonical folder it was opened from. The
/// app's own derivation — never a copy of it.
fn installation_key(root: &Path, workspace_id: ItemId) -> String {
    let canonical = std::fs::canonicalize(root).expect("the clone folder exists");
    pustman::scope_wiring::workspace_key(&workspace_id.to_string(), &canonical)
}

/// The app's keychain lookup for one installation (S43w's `scopes_for`): a scope's secret
/// `name` names one entry under this installation key and the scope's own id.
fn secret_keys(
    installation: String,
    scope_id: String,
) -> impl Fn(ScopeKind, &str) -> Option<SecretKey> {
    move |_kind, name| SecretKey::new(installation.as_str(), scope_id.as_str(), name).ok()
}

/// The fixture workspace: `pustman.json` alone, committed before B clones.
fn workspace_item(id: ItemId) -> Item {
    Item {
        body: ItemBody::Workspace(Workspace {
            id,
            name: "Shared workspace".to_string(),
            format_version: FORMAT_VERSION.to_string(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::new(),
        path: None,
    }
}

/// The fixture environment: `api_key` as a secret with no value of its own — the value
/// lives in the keychain only (REQ-VAR-SHARE-02).
fn environment_item(id: ItemId) -> Item {
    Item {
        body: ItemBody::Environment(Environment {
            id,
            name: "Shared".to_string(),
            variables: vec![Variable {
                key: SECRET_KEY.to_string(),
                value: None,
                kind: VariableKind::Secret,
                enabled: true,
                description: Some("each teammate supplies their own".to_string()),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("environments"),
        path: None,
    }
}

/// The fixture request: the shared request that references `{{api_key}}`.
fn request_item(id: ItemId) -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id,
            name: "Shared request".to_string(),
            method: Method::Get,
            url: "https://api.example.test/ping?key={{api_key}}".to_string(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
        dir: PathBuf::from("collections/users-api"),
        path: None,
    }
}

/// The environment item `id` holds in `snapshot`.
fn environment_of(snapshot: &WorkspaceSnapshot, id: ItemId) -> Environment {
    match &snapshot
        .items
        .get(&id)
        .expect("the environment is in the snapshot")
        .body
    {
        ItemBody::Environment(environment) => environment.clone(),
        other => panic!("item {id} is not an environment: {other:?}"),
    }
}

/// The request item `id` holds in `snapshot`, as the HTTP request it is.
fn request_of(snapshot: &WorkspaceSnapshot, id: ItemId) -> HttpRequest {
    match &snapshot
        .items
        .get(&id)
        .expect("the request is in the snapshot")
        .body
    {
        ItemBody::Request(Request::Http(request)) => request.clone(),
        other => panic!("item {id} is not an HTTP request: {other:?}"),
    }
}

/// Write one item into the workspace at `root`.
fn save(root: &Path, item: Item) {
    workspace_store::save_item(root, &item).expect("the item saves");
}

/// Give the repository its own identity, as `git commit` needs one.
fn configure_identity(root: &Path, name: &str, email: &str) {
    run_git(root, &["config", "user.name", name]);
    run_git(root, &["config", "user.email", email]);
}

/// Whether the `git` CLI is installed at all.
fn git_installed() -> bool {
    smol::block_on(git_command(Path::new("."), &["--version"]).output())
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Run one `git` command in `root`, answering its stdout; the test fails naming the
/// command and git's stderr when it does not succeed.
fn run_git(root: &Path, args: &[&str]) -> String {
    let output = smol::block_on(git_command(root, args).output()).expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git answers in UTF-8")
}

/// The `git` command for `root`, with the environment hygiene the `git` crate applies to
/// every command (S44a): `GIT_DIR` and its siblings name a repository of their own and
/// beat `-C`, and `GIT_TERMINAL_PROMPT=0` keeps git from prompting on the test's behalf.
fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
    ] {
        command.env_remove(variable);
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.args(args);
    command
}

/// Whether `needle` appears in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
