//! The sharing AC (PRD §6.3) end to end: two clones of one Git repository.
//!
//! Teammate A commits the workspace, then shares a request that uses `{{api_key}}` and an
//! environment whose `api_key` is a `Secret` **carrying the secret value**. Teammate B —
//! the same `pustman.json` id, a different folder — opens the workspace snapshot, watches
//! it, and pulls. The test proves the whole chain: within 1 s the request appears in B's
//! tree as an [`FsChange::ItemAdded`]; B's environment shows `api_key` as a secret with
//! no value; preparing the send in B fails with `UnresolvedSecret { names: ["api_key"] }`;
//! and the value reaches no commit and no file B can read.
//!
//! # The pull lands files only in watched directories (fix review)
//!
//! A's first commit — before B clones and starts the watcher — already carries an item in
//! each directory the share commit will add files to (`environments/`,
//! `collections/users-api/`). A pull that *creates* a directory races the watcher's own
//! recursive watch: the children are written before the new watch is armed, so the
//! watcher can only answer [`FsChange::Rescan`] for the directory and the files never
//! surface as `ItemAdded` (its `process_path` says so). Seeding both directories keeps
//! the pull to files inside already-watched directories, where `ItemAdded` is reliable —
//! the assertion the AC needs.
//!
//! # The value really flows through the save path (fix review)
//!
//! The environment goes through the real save path with the secret value in hand — the
//! model value of a `Secret`-kind variable plus the raw JSON an import keeps verbatim in
//! `extra` (REQ-MODEL-02) — and the leak scans then prove the sanitizing kept both out of
//! Git. The two carriers cover the two layers that must hold: `api_model`'s serialization
//! never writes a secret's `value`, and `workspace_store`'s scrub strips a secret out of
//! everything else (the `extra` map's raw leftover is only guarded by that scrub). A leak
//! scan that saw only a file-written fixture could never fail; this one fails whenever
//! either layer stops holding for what actually reaches `save_item`.
//!
//! The keychain half is the app's save path too: the write through the environment
//! editor's own [`SecretChange::set`] + [`AppliedChange`] — what `env_editor` calls —
//! against one [`MemorySecretStore`]; the reads through
//! [`pustman::scope_wiring::secret_key`]. The two spellings are the one
//! `secrets::SecretKey::new` constructor underneath, and the positive control below is
//! what proves they name one entry: the write lands on the key the read side composes, so
//! drifting apart fails the test.
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
//! # Fixtures and environment
//!
//! The secret value is obviously fake (`.agents/rules/tests.md`): `S3cr3t-Api-Key-456` is
//! leetspeak, not a credential. Only `MemorySecretStore` is used — never the OS keychain —
//! and every repository is a local bare repo in temp dirs; nothing touches the network.
//! The value never reaches a `Debug`/`Display` line: assertions name the sink, never the
//! value. Every `git` command is isolated from the developer's own git configuration (fix
//! review): no global or system config, no hooks, no signing, no prompts. The wait for
//! the watcher is a real-time bounded wait of 1 s started when `git pull` returns (the
//! step's "bounded wait" alternative to a GPUI test clock), like `workspace_store`'s own
//! watcher tests: the watcher debounces on real time in a background thread, and
//! preparing a send (`http_engine::prepare`) needs no GPUI context at all. When `git` is
//! missing the test skips **visibly** (libtest captures `eprintln!`, so the skip writes to
//! stderr directly) and panics outright under a *truthy* `CI`, where a missing `git` is a
//! broken image, not a reason to skip.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helper fns below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, Method, Request, Variable, VariableKind,
    Workspace,
};
use environment_picker::env_editor::EditTarget;
use environment_picker::secret_values::{AppliedChange, SecretChange};
use http_engine::{PipelineError, TransportError, prepare};
use secrets::{MemorySecretStore, SecretKey, SecretStore};
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
        if ci() {
            // Under CI a missing `git` is a broken image, not a reason to skip: silently
            // passing there would make the sharing AC vanish from CI entirely.
            panic!(
                "sharing_ac_two_clones: `git` is not installed; CI must provide it for the \
                 sharing AC (PRD §6.3)"
            );
        }
        // libtest captures `eprintln!`, so a local skip would be invisible: write to the
        // process's stderr directly, where a human running the suite can see it.
        std::io::stderr()
            .write_all(
                b"skipping sharing_ac_two_clones: `git` is not installed, so two clones of a \
                  repository cannot be made\n",
            )
            .expect("the test's stderr writes");
        return;
    }

    let scratch = tempfile::tempdir().expect("the scratch directory opens");
    run_git(scratch.path(), &["init", "--bare", "origin.git"]);

    // Clone A first, so the shared history exists before B's clone. The first commit
    // seeds both directories the share commit writes into (module docs: the watcher race).
    run_git(scratch.path(), &["clone", "origin.git", "team-a"]);
    let root_a = scratch.path().join("team-a");
    configure_identity(&root_a, "Teammate A", "a@example.test");

    let workspace_id = ItemId::new();
    let environment_id = ItemId::new();
    let request_id = ItemId::new();
    save(&root_a, workspace_item(workspace_id));
    save(&root_a, seed_environment_item());
    save(&root_a, seed_request_item());
    run_git(&root_a, &["add", "-A"]);
    run_git(&root_a, &["commit", "-m", "Add the shared workspace"]);
    run_git(&root_a, &["push", "-u", "origin", "HEAD"]);

    // Teammate B's clone, as it stands before A shares anything: the workspace and the
    // two seeded directories, watched from the very start.
    run_git(scratch.path(), &["clone", "origin.git", "team-b"]);
    let root_b = scratch.path().join("team-b");
    configure_identity(&root_b, "Teammate B", "b@example.test");

    // A: the request using `{{api_key}}`, and the environment the way the app saves it
    // when the user sets a secret value — the value in hand, through the real save path.
    let shared = environment_item(environment_id);
    save(&root_a, shared.clone());
    save(&root_a, request_item(request_id));

    // The keychain half of that same save, through the function the environment editor
    // itself writes with (`env_editor` calls `SecretChange::set`), against one shared
    // [`MemorySecretStore`] and before anything is committed.
    let keychain: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let installation_a = installation_key(&root_a, workspace_id);
    let scope_id = scope_id_of(&shared);
    let set = SecretChange::set(&installation_a, &scope_id, SECRET_KEY, SECRET)
        .expect("the fixture ids are valid keychain ids and the value is not empty");
    AppliedChange::apply(&set, &keychain).expect("A's secret stores under A's installation key");

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

    run_git(&root_b, &["pull"]);
    let deadline = Instant::now() + WAIT;

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

    // The leak scans first: the value went through the save path and must be in none of
    // the places a teammate could read it back (PRD §6.3 AC: `git diff` is clean).
    let history = run_git(&root_a, &["log", "-p", "--all"]);
    assert!(
        !contains(history.as_bytes(), SECRET.as_bytes()),
        "A's history leaked the secret value"
    );
    let commit = run_git(&root_a, &["show", "HEAD"]);
    assert!(
        !contains(commit.as_bytes(), SECRET.as_bytes()),
        "A's commit leaked the secret value"
    );
    for (path, bytes) in read_working_tree(&root_b) {
        assert!(
            !contains(&bytes, SECRET.as_bytes()),
            "B's working tree leaked the secret value in {}",
            path.display()
        );
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

    // The keying: B's own installation key reaches nothing, however identical the two
    // clones' committed `pustman.json` is. `key_a` is composed the *read* way
    // (`scope_wiring::secret_key`) while the write above went through the editor's
    // `SecretChange::set`: the positive control below proves the two spellings name one
    // entry — a drift between them fails it.
    let key_a = pustman::scope_wiring::secret_key(&installation_a, &scope_id, SECRET_KEY)
        .expect("the fixture ids are valid keychain ids");
    let installation_b = installation_key(&root_b, workspace_id);
    let key_b = pustman::scope_wiring::secret_key(&installation_b, &scope_id, SECRET_KEY)
        .expect("the fixture ids are valid keychain ids");
    assert_ne!(
        key_a, key_b,
        "clone B's keychain key must differ from clone A's"
    );

    let scopes = {
        let mut scopes = ScopeStack::new();
        scopes.push(ScopeKind::Environment, environment.variables.clone());
        scopes
    };

    // The positive control: A's own key resolves against this very store, so a failure
    // below can only be the keying, not an empty keychain.
    let lookup_a = secret_keys(installation_a, scope_id.clone());
    let prepared_a = prepare(
        &request,
        &[],
        &scopes,
        keychain.as_ref(),
        &lookup_a,
        &mut DynamicValues::new(),
    );
    assert!(
        prepared_a.is_ok(),
        "clone A's own installation key resolves the secret; the keychain is not empty: {:?}",
        prepared_a.err()
    );

    // The AC's send in B: preparing it fails as unresolved secret, naming api_key.
    let lookup_b = secret_keys(installation_b, scope_id);
    let prepared_b = prepare(
        &request,
        &[],
        &scopes,
        keychain.as_ref(),
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
}

/// Whether this run is CI. Only a *truthy* `CI` counts: a `CI=false` some local shell
/// profile exports must not turn the skip into a panic.
fn ci() -> bool {
    matches!(std::env::var("CI").as_deref(), Ok("true") | Ok("1"))
}

/// The installation key the app keeps one clone's local values and keychain entries under
/// (S43w): the `pustman.json` id bound to the canonical folder it was opened from. The
/// app's own derivation — never a copy of it.
fn installation_key(root: &Path, workspace_id: ItemId) -> String {
    let canonical = std::fs::canonicalize(root).expect("the clone folder exists");
    pustman::scope_wiring::workspace_key(&workspace_id.to_string(), &canonical)
}

/// The app's keychain lookup for one installation (S43w's `scopes_for`): a scope's secret
/// `name` names one entry under this installation key and the scope's own id — composed
/// by the app's own [`pustman::scope_wiring::secret_key`], never copied here.
fn secret_keys(
    installation: String,
    scope_id: String,
) -> impl Fn(ScopeKind, &str) -> Option<SecretKey> {
    move |_kind, name| pustman::scope_wiring::secret_key(&installation, &scope_id, name).ok()
}

/// The scope id the app keys an environment's local values and keychain entries by
/// (`env_editor`'s [`EditTarget`]): the environment's own item id.
fn scope_id_of(item: &Item) -> String {
    EditTarget::new(item.clone())
        .expect("an environment is a variable scope")
        .scope_id()
        .to_string()
}

/// The fixture workspace: `pustman.json`, committed before B clones.
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

/// The seed environment of the first commit: the item that puts `environments/` in
/// place — and therefore under the watcher — before the share commit writes into it
/// (module docs: the watcher race).
fn seed_environment_item() -> Item {
    Item {
        body: ItemBody::Environment(Environment {
            id: ItemId::new(),
            name: "Seed".to_string(),
            variables: Vec::new(),
            extra: BTreeMap::new(),
        }),
        dir: PathBuf::from("environments"),
        path: None,
    }
}

/// The seed request of the first commit: the item that puts `collections/users-api/` in
/// place — and therefore under the watcher — before the share commit writes into it
/// (module docs: the watcher race).
fn seed_request_item() -> Item {
    Item {
        body: ItemBody::Request(Request::Http(HttpRequest {
            id: ItemId::new(),
            name: "Seed request".to_string(),
            method: Method::Get,
            url: "https://api.example.test/health".to_string(),
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

/// The fixture environment the way the app saves it after the user sets a secret value: a
/// `Secret`-kind `api_key` carrying the value in the model — what reaches
/// [`workspace_store::save_item`] and what its sanitizing must keep out of the file — plus
/// the raw JSON an import keeps verbatim in `extra` (REQ-MODEL-02), whose only guard is
/// that same sanitizing (`workspace_store`'s own `a_secret_under_an_extra_key_is_scrubbed_from_an_environment`
/// pins the shape). The leak scans stay falsifiable at each layer this way.
fn environment_item(id: ItemId) -> Item {
    let mut extra = BTreeMap::new();
    extra.insert(
        "vars".to_string(),
        serde_json::json!([
            {
                "key": SECRET_KEY,
                "kind": "secret",
                "value": SECRET,
                "description": "raw import leftover"
            }
        ]),
    );
    Item {
        body: ItemBody::Environment(Environment {
            id,
            name: "Shared".to_string(),
            variables: vec![Variable {
                key: SECRET_KEY.to_string(),
                value: Some(SECRET.to_string()),
                kind: VariableKind::Secret,
                enabled: true,
                description: Some("each teammate supplies their own".to_string()),
                extra: BTreeMap::new(),
            }],
            extra,
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

/// Every file of B's working tree (`.git` aside), as (path relative to the root, bytes):
/// the checked-out files B can read. History is covered by the commit scans.
fn read_working_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files
}

/// Append every file under `dir` to `files`, descending into subdirectories but never
/// into `.git`.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("the directory reads") {
        let entry = entry.expect("the directory entry reads");
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != std::ffi::OsStr::new(".git") {
                collect_files(root, &path, files);
            }
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("the file is under the root")
                .to_path_buf();
            files.push((relative, std::fs::read(&path).expect("the file reads")));
        }
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

/// The `git` command for `root`, isolated from the developer's own git configuration (fix
/// review) and with the environment hygiene the `git` crate applies to every command
/// (S44a): `GIT_DIR` and its siblings name a repository of their own and beat `-C`, and
/// `GIT_TERMINAL_PROMPT=0` keeps git from prompting on the test's behalf. No global or
/// system config and no injected config (`GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`) —
/// so no signing key, default branch, template, hook or `-c` of theirs shapes the fixture
/// repos — and the local-file protocol allowed for the clones.
fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    command.arg("-c").arg("init.defaultBranch=main");
    command.arg("-c").arg("commit.gpgsign=false");
    command.arg("-c").arg("core.hooksPath=/dev/null");
    command.arg("-c").arg("protocol.file.allow=always");
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        command.env_remove(variable);
    }
    command.env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
    command.env("GIT_CONFIG_NOSYSTEM", "1");
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
