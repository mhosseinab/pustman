//! The variable-scope wire (S43w): the opened workspace's globals, its active environment
//! and each request's collection become the scopes every open [`RequestEditor`] resolves
//! `{{var}}` against (REQ-VAR-02/08), and the OS keychain becomes where its secret-kind
//! variables resolve from at send (REQ-VAR-SHARE-02). The same state feeds the
//! environment picker, the quick-look and the environment editor, so all four see one
//! workspace and one active environment.
//!
//! What it follows, and what each change re-resolves:
//! - [`WorkspaceOpen`]'s events: a workspace opening (its key, local values and persisted
//!   active environment are then read off the foreground) or its snapshot changing.
//! - [`SelectorInputs`]: the picker's confirmed choice is the active environment.
//! - [`EnvEditorInputs`]: the environment editor refreshes them after a save, which is
//!   how a changed shared or local value — or a set or removed secret — reaches the open
//!   editors.
//! - [`SecretInputs`]: the keychain every editor's [`SecretSources`] reads at send.
//!
//! `Decision:` the scope rules are `variables`' own: the stack is built with
//! [`ScopeStack::push`], and a local value is applied to a name only when its winning
//! variable (the one [`scope_kind_of`] names) lives in the scope that local value was
//! saved for — local values are per scope in storage but per name in the stack. A secret
//! never takes a value from storage: its only value is the keychain's, read by
//! `http_engine::prepare` at send through [`SecretSources`].
//!
//! `Decision:` which secrets the keychain holds is checked off the foreground, and
//! [`SecretSources`] names a key only for those. The pre-send check (#198) then warns
//! about a secret with nothing stored — and "Send anyway" fails it as unresolved —
//! without touching the keychain on the foreground. A keychain that cannot be reached
//! counts as holding the entry, so the send reports that failure itself. (fix 2) Every
//! read can be an OS prompt on an unsigned build, so the check runs only when the set of
//! secret keys changes (the workspace's key, or a secret variable added, removed or
//! renamed) or another keychain is installed — never on an environment switch or an
//! unrelated disk change. What the app itself writes to the keychain is learnt from the
//! write: the wire installs the keychain behind [`TrackedKeychain`], which notes each
//! key set or removed (never a value). The set is one shared cell per open workspace
//! ([`StoredSecrets`]), read at call time (S43x): a snapshot copied into each lookup
//! would freeze a failed send's key naming before the very save that makes its secret
//! resolvable, and the `UnresolvedSecret` panel's re-check — which resolves against that
//! captured send — could then never offer Resend.
//!
//! `Decision:` (fix 1) local values, the active environment's selection and keychain
//! entries live under [`workspace_key`]: the workspace's `pustman.json` id bound to the
//! canonical folder it was opened from on this installation. The id alone is committed to
//! a shared repo, so a cloned or crafted folder carrying it would otherwise load this
//! folder's local values and resolve its secrets into requests that folder controls.
//! History, cookies and restored tabs keep [`crate::WORKSPACE_ID`]. Until the folder's key
//! is known, the picker's, quick-look's and environment editor's inputs are removed, so
//! none of them can act on the previous folder's.
//!
//! `Decision:` (fix 2, fix 3, fix 4) a request editor is bound to the open workspace only
//! when that workspace's snapshot holds its request with the same URL template and the
//! same settings its draft has — an id alone can be reused by a crafted folder or a fork
//! to aim a restored draft at its own host, or to turn TLS verification off or keep the
//! Authorization header across redirects — or when it was built as a fresh request and
//! still is one when it binds (nothing but a name and a method, what New Request builds:
//! whatever host and variables it gets are typed while this workspace is open). A
//! restored draft with no URL but with headers, a body or auth is not fresh: those came
//! from wherever it was saved; nor is a fresh one filled in before any folder opened.
//! Restored tabs and history entries are saved with no folder, so one from folder A,
//! reopened while B is open, stays unbound until a snapshot holding its request opens.
//! Once bound, an editor keeps that workspace: a tab left open from folder A after folder
//! B opens gets no scopes and no keychain. Either way its secrets stay unresolved and the
//! pre-send check warns, rather than resolving B's secrets into a request to A's host.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use api_model::{
    Auth, Body, Environment, HttpRequest, ItemId, Request, RequestSettings, VariableKind,
};
use environment_picker::env_editor::{self, EditTarget, EnvEditorInputs};
use environment_picker::quick_look::{self, QuickLookInputs};
use environment_picker::secret_values::{self, SecretInputs};
use environment_picker::selector::{self, SelectorInputs};
use gpui::{App, Entity, EntityId, Global, Subscription, Task, WeakEntity};
use request_editor::{RequestEditor, SecretSources, scope_kind_of};
use secrets::{KeyringStore, SecretError, SecretKey, SecretStore, SecretValue};
use sha2::{Digest, Sha256};
use storage::{LocalStore, LocalValue};
use variables::{ScopeKind, ScopeStack};
use workspace::{Workspace, WorkspaceOpen, WorkspaceOpenEvent};
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::{AppStore, StartupProblems};

/// What a failed read of the workspace's local values or active environment says. Fixed
/// text: `storage`'s error can quote a row, and a row can hold a value.
const LOAD_FAILURE_MESSAGE: &str = "The workspace's local variable values and active environment could not be read — \
     variables resolve from the shared values only";

/// What an unreadable globals list in `pustman.json` says.
const GLOBALS_FAILURE_MESSAGE: &str =
    "The workspace's globals could not be read — requests resolve without them";

/// What a workspace folder that cannot be resolved to its canonical path says: without
/// it there is no key for its local values, selection and secrets.
const FOLDER_FAILURE_MESSAGE: &str = "The workspace folder's location could not be resolved — its local values, \
     active environment and keychain secrets are unavailable";

/// What a window or workspace the wire cannot follow says: its requests resolve with no
/// workspace variables at all.
const WIRE_FAILURE_MESSAGE: &str =
    "The workspace's variables could not be loaded — requests resolve without them";

/// What a secret variable whose name the keychain cannot hold says, before the name.
const INVALID_SECRET_PREFIX: &str = "The secret variable ";

/// ... and after it: it stays unresolved, so a send warns before it goes.
const INVALID_SECRET_SUFFIX: &str =
    " cannot be kept in the OS keychain — requests leave it unresolved";

/// The workspace key names are checked against before the real one is known.
const UNKEYED_WORKSPACE: &str = "unkeyed";

/// The longest `pustman.json` id [`workspace_key`] uses as is (a ULID is 26 characters):
/// with the `.` and the folder's 32-character digest, the key stays within 64.
const MAX_PLAIN_WORKSPACE_ID: usize = 31;

/// How many bytes of a SHA-256 digest [`workspace_key`] keeps: 128 bits, hex-encoded.
const DIGEST_PREFIX_BYTES: usize = 16;

/// One opened workspace as the wire tells them apart: its `pustman.json` id and the
/// folder it was opened from. Known as soon as the snapshot is.
#[derive(Clone, PartialEq)]
struct WorkspaceIdentity {
    id: String,
    root: PathBuf,
}

/// What makes a draft the request a snapshot holds (module docs): its URL template and
/// its settings — what it sends to, and how.
#[derive(Clone, PartialEq)]
struct RequestTarget {
    url: String,
    settings: Option<RequestSettings>,
}

impl RequestTarget {
    fn of(request: &HttpRequest) -> Self {
        Self {
            url: request.url.clone(),
            settings: request.settings.clone(),
        }
    }
}

/// What binding reads of an editor's draft, taken without cloning its body.
struct DraftBinding {
    id: ItemId,
    target: RequestTarget,
    fresh: bool,
}

impl DraftBinding {
    fn of(draft: &HttpRequest) -> Self {
        Self {
            id: draft.id,
            target: RequestTarget::of(draft),
            fresh: is_fresh_request(draft),
        }
    }
}

/// One request editor and the workspace it belongs to (module docs): `None` until a
/// workspace whose snapshot holds its request at the URL and settings its draft has
/// opens — or, for an editor built as a fresh request (`built_fresh`) that still is one,
/// until any workspace opens.
struct WiredEditor {
    editor: WeakEntity<RequestEditor>,
    workspace: Option<WorkspaceIdentity>,
    built_fresh: bool,
}

impl WiredEditor {
    /// Bind this editor, whose draft is `draft`, to `open` when it may be (module docs);
    /// whether it belongs to it.
    fn bind(&mut self, draft: &DraftBinding, open: &OpenScopes) -> bool {
        if self.workspace.is_none()
            && ((self.built_fresh && draft.fresh)
                || open.requests.get(&draft.id) == Some(&draft.target))
        {
            self.workspace = Some(open.identity.clone());
        }
        self.workspace.as_ref() == Some(&open.identity)
    }
}

/// Whether `draft` is what `NewRequest` builds: nothing but a name and a method, so
/// nothing in it was saved anywhere else — every host and `{{var}}` it ever gets is
/// typed while a workspace is open.
fn is_fresh_request(draft: &HttpRequest) -> bool {
    draft.url.trim().is_empty()
        && draft.params.is_empty()
        && draft.path_variables.is_empty()
        && draft.headers.is_empty()
        && matches!(draft.body, Body::None)
        && matches!(draft.auth, Auth::None)
        && draft.settings.is_none()
        && draft.examples.is_empty()
        && draft.extra.is_empty()
}

/// The app's keychain as the wire installs it (module docs): every call goes to the
/// keychain it wraps, and each key a write sets or removes is noted — the key and whether
/// it now holds an entry, never the value.
struct TrackedKeychain {
    inner: Arc<dyn SecretStore>,
    writes: Mutex<Vec<(SecretKey, bool)>>,
}

impl TrackedKeychain {
    fn note(&self, key: &SecretKey, stored: bool) {
        self.writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((key.clone(), stored));
    }

    /// The writes noted since the last call, oldest first.
    fn take_writes(&self) -> Vec<(SecretKey, bool)> {
        std::mem::take(&mut *self.writes.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl SecretStore for TrackedKeychain {
    fn get(&self, key: &SecretKey) -> Result<SecretValue, SecretError> {
        self.inner.get(key)
    }

    fn set(&self, key: &SecretKey, value: &SecretValue) -> Result<(), SecretError> {
        self.inner.set(key, value)?;
        self.note(key, true);
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.inner.delete(key)?;
        self.note(key, false);
        Ok(())
    }
}

/// The app-wide wire: the editors it pushes scopes into and the open workspace's scopes.
#[derive(Default)]
struct ScopeWiring {
    started: bool,
    /// Every request editor built since `init`; dead handles are dropped on each push.
    editors: Vec<WiredEditor>,
    open: Option<OpenScopes>,
    /// The read of the open workspace's key and stored state in flight: a newer open
    /// replaces — cancels — it, so an older workspace's values can never land on a newer one.
    loading: Option<Task<()>>,
    /// The keychain presence check in flight; a newer check replaces it.
    checking: Option<Task<()>>,
    /// The secret keys the check in flight reads: the same set is not read twice at once.
    checking_secrets: Option<HashSet<SecretKey>>,
    /// The keychain writes taken while that check is in flight, oldest first: its landing
    /// folds them into what it read, which may predate them.
    writes_during_check: Vec<(SecretKey, bool)>,
    /// The secret keys the last landed presence check read: the same set is not read
    /// again (module docs).
    checked_secrets: Option<HashSet<SecretKey>>,
    /// The keychain as the wire installed it, whose writes it follows.
    keychain: Option<Arc<TrackedKeychain>>,
    /// Bumped by every selection the user makes: a stored-state read started before one
    /// does not overwrite it.
    selection_generation: u64,
    /// Per scope id, bumped by every environment-editor save that changed that scope's
    /// local values: a stored-state read started before one does not overwrite them, and
    /// still lands every other scope's.
    locals_generations: BTreeMap<String, u64>,
    /// The problems already shown for the open workspace: each is shown once per open.
    reported: BTreeSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl Global for ScopeWiring {}

impl ScopeWiring {
    /// Cancel the presence check in flight and forget the last one: the next check reads
    /// the keychain again.
    fn forget_secret_checks(&mut self) {
        self.checking = None;
        self.checking_secrets = None;
        self.writes_during_check.clear();
        self.checked_secrets = None;
    }
}

/// The secret keys the keychain holds an entry for (module docs): one shared cell per
/// open workspace (S43x), read by every [`SecretKeyLookup`](http_engine::SecretKeyLookup)
/// closure at call time. A snapshot — `Arc<HashSet<_>>` cloned into each closure — went
/// stale behind the very save that makes a secret resolvable: the `UnresolvedSecret`
/// panel's captured failed send then keeps answering "not stored" for ever and its
/// re-check never offers Resend. Keys only: nothing reads a value through this, and the
/// lock is never held across an `.await`.
#[derive(Clone, Default)]
struct StoredSecrets(Arc<RwLock<HashSet<SecretKey>>>);

impl StoredSecrets {
    /// Whether the keychain holds an entry for `key`.
    fn contains(&self, key: &SecretKey) -> bool {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(key)
    }

    /// Fold `writes` — one entry set or removed per key — into the cell in place;
    /// whether that changed it.
    fn apply_writes(&self, writes: impl IntoIterator<Item = (SecretKey, bool)>) -> bool {
        let mut stored = self.0.write().unwrap_or_else(PoisonError::into_inner);
        let mut changed = false;
        for (key, is_stored) in writes {
            changed |= if is_stored {
                stored.insert(key)
            } else {
                stored.remove(&key)
            };
        }
        changed
    }

    /// Replace the cell with what a presence check found; whether it differed.
    fn replace_with(&self, keys: HashSet<SecretKey>) -> bool {
        let mut stored = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if *stored == keys {
            return false;
        }
        *stored = keys;
        true
    }
}

/// One opened workspace's scopes. [`EditTarget`] names each scope's id and variables —
/// the same ids the environment editor saves local values and keychain entries under.
#[derive(Clone)]
struct OpenScopes {
    identity: WorkspaceIdentity,
    /// [`workspace_key`], once read off the foreground; nothing keyed is read or
    /// installed before.
    key: Option<String>,
    /// `None` when the workspace has no readable globals list.
    globals: Option<EditTarget>,
    /// The environments in the picker's order (name, then id), each with its scope.
    environments: Vec<(Environment, EditTarget)>,
    collections: BTreeMap<ItemId, EditTarget>,
    /// Which collection each request belongs to, through any folders.
    collection_of: BTreeMap<ItemId, ItemId>,
    /// Every HTTP request the snapshot holds, with its URL template and settings: the
    /// editors that may bind to it (module docs).
    requests: BTreeMap<ItemId, RequestTarget>,
    local_values: Vec<LocalValue>,
    active: Option<ItemId>,
    /// The secret keys the keychain holds an entry for (module docs) — shared live
    /// ([`StoredSecrets`]), so every lookup closure built from these scopes reads it at
    /// call time.
    stored_secrets: StoredSecrets,
}

impl OpenScopes {
    fn active_environment(&self) -> Option<&(Environment, EditTarget)> {
        let active = self.active?;
        self.environments
            .iter()
            .find(|(environment, _)| environment.id == active)
    }

    fn targets(&self) -> impl Iterator<Item = &EditTarget> {
        self.globals
            .iter()
            .chain(self.environments.iter().map(|(_, target)| target))
            .chain(self.collections.values())
    }

    fn target_mut(&mut self, scope_id: &str) -> Option<&mut EditTarget> {
        if let Some(globals) = self.globals.as_mut()
            && globals.scope_id() == scope_id
        {
            return Some(globals);
        }
        if let Some((_, target)) = self
            .environments
            .iter_mut()
            .find(|(_, target)| target.scope_id() == scope_id)
        {
            return Some(target);
        }
        self.collections
            .values_mut()
            .find(|target| target.scope_id() == scope_id)
    }

    fn local_values_of(&self, scope_id: &str) -> Vec<LocalValue> {
        let mut values: Vec<LocalValue> = self
            .local_values
            .iter()
            .filter(|value| value.scope_id == scope_id)
            .cloned()
            .collect();
        values.sort_by(|left, right| left.key.cmp(&right.key));
        values
    }

    /// Whether `workspace_id` — an installed input's — is this workspace's key.
    fn is_keyed(&self, workspace_id: &str) -> bool {
        self.key.as_deref() == Some(workspace_id)
    }
}

/// The installation-local key one opened workspace's local values, active-environment
/// selection and keychain entries live under: its `pustman.json` id bound to the folder
/// it was opened from, `canonical_root` (module docs). Two folders never share a key even
/// when one carries a copy of the other's `pustman.json`.
///
/// The folder is a fixed-length digest (the first 128 bits of its SHA-256, hex): however
/// deep the path, the keychain account stays within the OS's account-name limits. The
/// id is untrusted file content: it is used as is only when it is a short run of ASCII
/// letters, digits, `-` and `_` (a ULID is), and is digested otherwise — cut to the same
/// length and joined with `:` instead of `.`, so it never reads as a plain id. Either way
/// the key is at most 64 characters and carries no `/` or NUL (a [`SecretKey`] id must
/// not).
pub fn workspace_key(workspace_id: &str, canonical_root: &Path) -> String {
    let plain_id = !workspace_id.is_empty()
        && workspace_id.len() <= MAX_PLAIN_WORKSPACE_ID
        && workspace_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    let root = digest_hex(canonical_root.as_os_str().as_encoded_bytes());
    if plain_id {
        return format!("{workspace_id}.{root}");
    }
    let mut id = digest_hex(workspace_id.as_bytes());
    id.truncate(MAX_PLAIN_WORKSPACE_ID);
    format!("{id}:{root}")
}

/// The first [`DIGEST_PREFIX_BYTES`] of `bytes`' SHA-256, hex-encoded.
fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(
        digest
            .get(..DIGEST_PREFIX_BYTES)
            .unwrap_or(digest.as_slice()),
    )
}

/// The keychain entry one secret variable keeps its value under on this installation:
/// the workspace's key, the scope's id and the variable's name. This crate's key *reads*
/// compose through here (`check_stored_secrets`, `scopes_for`); the environment editor's
/// key *writes* compose through `environment_picker::secret_values::SecretChange::set`,
/// which cannot call up into this crate. Both spellings bottom out in the one
/// `secrets::SecretKey::new` constructor, and S47's sharing proof writes through the
/// editor and reads through here against one store, so the two spellings drifting apart
/// fails that test.
pub fn secret_key(
    workspace_key: &str,
    scope_id: &str,
    variable_key: &str,
) -> Result<SecretKey, SecretError> {
    SecretKey::new(workspace_key, scope_id, variable_key)
}

/// Install the OS keychain as the app's secret store ([`SecretInputs`]): where the
/// environment editor saves secret values and every request editor resolves them from.
/// Called where the app's local store lands, beside the other installs.
pub fn install_keychain(cx: &mut App) {
    let store: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
    secret_values::install_inputs(cx, SecretInputs { store });
}

/// Wire the scopes into the app (module docs). Runs once per app.
pub fn init(cx: &mut App) {
    if cx.default_global::<ScopeWiring>().started {
        return;
    }
    cx.default_global::<ScopeWiring>().started = true;
    let subscriptions = [
        cx.observe_new::<RequestEditor>(|editor, _window, cx| {
            let entity = cx.entity();
            let request_id = editor.draft().id;
            let draft = DraftBinding::of(editor.draft());
            let mut wired = WiredEditor {
                editor: entity.downgrade(),
                workspace: None,
                built_fresh: draft.fresh,
            };
            let open = cx.global::<ScopeWiring>().open.clone();
            let bound = open.as_ref().is_some_and(|open| wired.bind(&draft, open));
            cx.global_mut::<ScopeWiring>().editors.push(wired);
            let Some(open) = open.filter(|_| bound) else {
                return;
            };
            let (scopes, sources) = scopes_for(&open, request_id, cx);
            editor.set_scopes(scopes, cx);
            editor.set_secret_sources(sources, cx);
        }),
        cx.observe_new::<Workspace>(|_workspace, _window, cx| {
            let workspace = cx.entity();
            // Deferred: `WorkspaceOpen` attaches to the window in its own observer.
            cx.defer(move |cx| watch_workspace(&workspace, cx));
        }),
        cx.observe_global::<AppStore>(load_open_workspace),
        cx.observe_global::<SelectorInputs>(follow_selector),
        cx.observe_global::<EnvEditorInputs>(follow_env_editor),
        cx.observe_global::<SecretInputs>(follow_keychain),
    ];
    cx.global_mut::<ScopeWiring>()
        ._subscriptions
        .extend(subscriptions);
    // A keychain installed before the wire is put behind it too.
    follow_keychain(cx);
}

/// Follow `workspace`'s open folder: every open and snapshot change re-reads its scopes.
fn watch_workspace(workspace: &Entity<Workspace>, cx: &mut App) {
    let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
        StartupProblems::push(cx, WIRE_FAILURE_MESSAGE);
        return;
    };
    let subscription = cx.subscribe(&opener, |opener, event: &WorkspaceOpenEvent, cx| {
        let opened = matches!(event, WorkspaceOpenEvent::Opened { .. });
        open_changed(&opener, opened, cx);
    });
    cx.global_mut::<ScopeWiring>()
        ._subscriptions
        .push(subscription);
    open_changed(&opener, true, cx);
}

/// Show `problem` unless it was already shown since the workspace opened.
fn report(cx: &mut App, problem: String) {
    if cx
        .global_mut::<ScopeWiring>()
        .reported
        .insert(problem.clone())
    {
        StartupProblems::push(cx, problem);
    }
}

/// The open workspace or its snapshot changed (`opened`: a folder was opened): rebuild
/// the scopes from the snapshot. A different workspace starts with no key, no active
/// environment and no local values until its own are read; the same workspace keeps all
/// three.
fn open_changed(opener: &Entity<WorkspaceOpen>, opened: bool, cx: &mut App) {
    if opened {
        cx.global_mut::<ScopeWiring>().reported.clear();
    }
    let Some(snapshot) = opener.read(cx).snapshot() else {
        return;
    };
    let Some(mut scopes) = scopes_of(snapshot) else {
        report(cx, WIRE_FAILURE_MESSAGE.to_string());
        close_workspace(cx);
        return;
    };
    let globals_unreadable = scopes.globals.is_none() && has_globals_field(snapshot);
    let previous = cx.global_mut::<ScopeWiring>().open.take();
    match previous {
        Some(previous) if previous.identity == scopes.identity => {
            scopes.key = previous.key;
            scopes.local_values = previous.local_values;
            scopes.active = previous.active;
            scopes.stored_secrets = previous.stored_secrets;
            cx.global_mut::<ScopeWiring>().open = Some(scopes);
            apply(cx);
            check_stored_secrets(cx);
        }
        _ => {
            // Reported once per open, not on every snapshot change of the same one.
            if globals_unreadable {
                report(cx, GLOBALS_FAILURE_MESSAGE.to_string());
            }
            remove_inputs(cx);
            let wiring = cx.global_mut::<ScopeWiring>();
            wiring.open = Some(scopes);
            wiring.forget_secret_checks();
            apply(cx);
            load_open_workspace(cx);
        }
    }
}

/// No workspace the wire can follow: drop its scopes and inputs, and leave every editor
/// with none rather than with the last workspace's.
fn close_workspace(cx: &mut App) {
    remove_inputs(cx);
    let wiring = cx.global_mut::<ScopeWiring>();
    wiring.open = None;
    wiring.loading = None;
    wiring.forget_secret_checks();
    for editor in live_editors(cx) {
        editor.update(cx, |editor, cx| {
            editor.set_scopes(ScopeStack::new(), cx);
            editor.set_secret_sources(SecretSources::default(), cx);
        });
    }
}

/// Remove the picker's, quick-look's and environment editor's inputs: until the next
/// workspace's key is known, none of them may act on the previous workspace's.
fn remove_inputs(cx: &mut App) {
    if cx.has_global::<SelectorInputs>() {
        drop(cx.remove_global::<SelectorInputs>());
    }
    if cx.has_global::<QuickLookInputs>() {
        drop(cx.remove_global::<QuickLookInputs>());
    }
    if cx.has_global::<EnvEditorInputs>() {
        drop(cx.remove_global::<EnvEditorInputs>());
    }
}

/// Whether `pustman.json` carries a globals field at all — so an unreadable one is
/// reported, and an absent one (no globals yet) is not.
fn has_globals_field(snapshot: &WorkspaceSnapshot) -> bool {
    snapshot.items.values().any(|item| match &item.body {
        ItemBody::Workspace(workspace) => workspace.extra.contains_key("variables"),
        _ => false,
    })
}

/// The scopes `snapshot` holds, or `None` when it has no workspace document.
fn scopes_of(snapshot: &WorkspaceSnapshot) -> Option<OpenScopes> {
    let workspace_item = snapshot
        .items
        .values()
        .find(|item| matches!(item.body, ItemBody::Workspace(_)))?;
    let workspace_id = workspace_item.id()?.to_string();
    let globals = EditTarget::new(workspace_item.clone()).ok();
    let mut environments = Vec::new();
    let mut collections = BTreeMap::new();
    for (id, item) in &snapshot.items {
        match &item.body {
            ItemBody::Environment(environment) => {
                if let Ok(target) = EditTarget::new(item.clone()) {
                    environments.push((environment.clone(), target));
                }
            }
            ItemBody::Collection(_) => {
                if let Ok(target) = EditTarget::new(item.clone()) {
                    collections.insert(*id, target);
                }
            }
            _ => {}
        }
    }
    environments.sort_by(|(left, _), (right, _)| {
        (left.name.as_str(), left.id).cmp(&(right.name.as_str(), right.id))
    });
    Some(OpenScopes {
        identity: WorkspaceIdentity {
            id: workspace_id,
            root: snapshot.root.clone(),
        },
        key: None,
        globals,
        environments,
        collections,
        collection_of: collection_of(snapshot),
        requests: snapshot
            .items
            .iter()
            .filter_map(|(id, item)| match &item.body {
                ItemBody::Request(Request::Http(request)) => {
                    Some((*id, RequestTarget::of(request)))
                }
                _ => None,
            })
            .collect(),
        local_values: Vec::new(),
        active: None,
        stored_secrets: StoredSecrets::default(),
    })
}

/// Each request's collection: up its `order` parents (`collections_panel`'s parent map,
/// the tree's own parentage) until a collection. A cycle in an untrusted workspace stops
/// after as many steps as there are items.
fn collection_of(snapshot: &WorkspaceSnapshot) -> BTreeMap<ItemId, ItemId> {
    let parents = collections_panel::crud::parent_map(&snapshot.items);
    let mut result = BTreeMap::new();
    for (id, item) in &snapshot.items {
        if !matches!(item.body, ItemBody::Request(_)) {
            continue;
        }
        let mut current = *id;
        for _ in 0..snapshot.items.len() {
            let Some(parent) = parents.get(&current) else {
                break;
            };
            match snapshot.items.get(parent).map(|item| &item.body) {
                Some(ItemBody::Collection(_)) => {
                    result.insert(*id, *parent);
                    break;
                }
                Some(_) => current = *parent,
                None => break,
            }
        }
    }
    result
}

/// Read the open workspace's key (when not known yet), then its local values and stored
/// active environment, all off the foreground, pushing the scopes as each lands. Without a
/// store yet, stop after the key; the store landing runs this again.
fn load_open_workspace(cx: &mut App) {
    let wiring = cx.global::<ScopeWiring>();
    let Some((identity, known_key)) = wiring
        .open
        .as_ref()
        .map(|open| (open.identity.clone(), open.key.clone()))
    else {
        return;
    };
    // Taken now, not when the read starts: a choice confirmed before the task first runs
    // is already newer than what it will read.
    let (selection_generation, locals_generations) = (
        wiring.selection_generation,
        wiring.locals_generations.clone(),
    );
    let store = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0));
    let task = cx.spawn(async move |cx| {
        let key = match known_key {
            Some(key) => key,
            None => {
                let resolved = cx
                    .background_executor()
                    .spawn({
                        let identity = identity.clone();
                        async move {
                            std::fs::canonicalize(&identity.root)
                                .map(|canonical| workspace_key(&identity.id, &canonical))
                        }
                    })
                    .await;
                let installed = cx.update(|cx| install_key(&identity, resolved, cx));
                let Some(key) = installed else {
                    return;
                };
                key
            }
        };
        let Some(store) = store else {
            return;
        };
        let state = cx
            .background_executor()
            .spawn({
                let key = key.clone();
                async move { read_workspace_state(&store, &key) }
            })
            .await;
        cx.update(|cx| {
            let wiring = cx.global_mut::<ScopeWiring>();
            let take_selection = wiring.selection_generation == selection_generation;
            // The scopes whose local values were saved while this read was in flight.
            let saved_since: BTreeSet<String> = wiring
                .locals_generations
                .iter()
                .filter(|(scope_id, generation)| {
                    locals_generations.get(*scope_id) != Some(generation)
                })
                .map(|(scope_id, _)| scope_id.clone())
                .collect();
            let Some(open) = wiring.open.as_mut() else {
                return;
            };
            if open.identity != identity || !open.is_keyed(&key) {
                return;
            }
            match state {
                Ok((local_values, selection)) => {
                    // A choice or a save the user made while this read was in flight is
                    // newer than what it read: that part of the read is discarded — for
                    // local values, only the saved scopes'.
                    let mut merged: Vec<LocalValue> = local_values
                        .into_iter()
                        .filter(|value| !saved_since.contains(&value.scope_id))
                        .collect();
                    merged.extend(
                        open.local_values
                            .iter()
                            .filter(|value| saved_since.contains(&value.scope_id))
                            .cloned(),
                    );
                    open.local_values = merged;
                    if take_selection {
                        open.active = selection.and_then(|selected| {
                            open.environments
                                .iter()
                                .map(|(environment, _)| environment.id)
                                .find(|id| id.to_string() == selected)
                        });
                    }
                    apply(cx);
                }
                Err(_) => {
                    apply(cx);
                    report(cx, LOAD_FAILURE_MESSAGE.to_string());
                }
            }
        });
    });
    cx.global_mut::<ScopeWiring>().loading = Some(task);
}

/// The open workspace's key landed: keep it if the same workspace is still open, then
/// install the inputs and push the scopes keyed by it.
fn install_key(
    identity: &WorkspaceIdentity,
    resolved: std::io::Result<String>,
    cx: &mut App,
) -> Option<String> {
    let open = cx.global_mut::<ScopeWiring>().open.as_mut()?;
    if &open.identity != identity {
        return None;
    }
    match resolved {
        Ok(key) => {
            open.key = Some(key.clone());
            apply(cx);
            check_stored_secrets(cx);
            Some(key)
        }
        Err(_) => {
            // Fixed text: the error quotes the folder's path.
            report(cx, FOLDER_FAILURE_MESSAGE.to_string());
            // Its secrets stay unresolved, but one whose name no keychain could hold is
            // still named.
            check_stored_secrets(cx);
            None
        }
    }
}

/// The workspace's non-secret local values and its stored active environment id.
fn read_workspace_state(
    store: &Mutex<LocalStore>,
    workspace_key: &str,
) -> anyhow::Result<(Vec<LocalValue>, Option<String>)> {
    let store = store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))?;
    let local_values = store.local_values().list(workspace_key)?;
    let key = selector::selection_key(workspace_key);
    let selection = store
        .kv()
        .list()?
        .into_iter()
        .find(|row| row.key == key)
        .map(|row| row.value);
    Ok((local_values, selection))
}

/// Check off the foreground which of the open workspace's secret-kind variables the
/// keychain holds an entry for, and re-resolve when that changed — only when the set of
/// secret keys differs from the last check's (module docs). Only whether an entry exists
/// is kept; every value read is dropped on the spot. A secret whose name the keychain
/// cannot hold is reported (once per open) and stays unresolved.
fn check_stored_secrets(cx: &mut App) {
    let Some(open) = cx.global::<ScopeWiring>().open.as_ref() else {
        return;
    };
    let workspace_key = open.key.clone();
    let identity = open.identity.clone();
    // Before the key is known — or when it cannot be — names are checked against a
    // stand-in: a real key is never empty and carries no `/` or NUL, so whether a name
    // can be kept does not depend on it, and an unkeyable one is reported either way.
    let key_for_names = workspace_key.as_deref().unwrap_or(UNKEYED_WORKSPACE);
    let mut keys = HashSet::new();
    let mut unkeyable = BTreeSet::new();
    for target in open.targets() {
        for variable in target.variables() {
            if variable.kind != VariableKind::Secret {
                continue;
            }
            match secret_key(key_for_names, target.scope_id(), variable.key.as_str()) {
                Ok(key) => {
                    keys.insert(key);
                }
                Err(_) => {
                    unkeyable.insert(variable.key.clone());
                }
            }
        }
    }
    for name in unkeyable {
        // The name can hold a NUL — the reason it was refused — which no toast shows.
        let shown = name.replace('\0', "\u{fffd}");
        report(
            cx,
            format!("{INVALID_SECRET_PREFIX}`{shown}`{INVALID_SECRET_SUFFIX}"),
        );
    }
    let Some(workspace_key) = workspace_key else {
        return;
    };
    let Some(store) = secret_values::store(cx) else {
        return;
    };
    let wiring = cx.global_mut::<ScopeWiring>();
    if wiring.checked_secrets.as_ref() == Some(&keys)
        || (wiring.checking.is_some() && wiring.checking_secrets.as_ref() == Some(&keys))
    {
        return;
    }
    wiring.checking_secrets = Some(keys.clone());
    wiring.writes_during_check.clear();
    let task = cx.spawn(async move |cx| {
        let checked = keys.clone();
        let stored = cx
            .background_executor()
            .spawn(async move {
                keys.into_iter()
                    .filter(|key| !matches!(store.get(key), Err(SecretError::NotFound { .. })))
                    .collect::<HashSet<SecretKey>>()
            })
            .await;
        cx.update(|cx| land_secret_check(&identity, &workspace_key, checked, stored, cx));
    });
    cx.global_mut::<ScopeWiring>().checking = Some(task);
}

/// A presence check of `checked` landed with `stored`, the keys among them the keychain
/// held: fold in what the app wrote to the keychain since the check started, keep it if
/// the same workspace is still open, and re-resolve when that changed anything.
fn land_secret_check(
    identity: &WorkspaceIdentity,
    workspace_key: &str,
    checked: HashSet<SecretKey>,
    mut stored: HashSet<SecretKey>,
    cx: &mut App,
) {
    take_secret_writes(cx);
    let wiring = cx.global_mut::<ScopeWiring>();
    let writes = std::mem::take(&mut wiring.writes_during_check);
    wiring.checking_secrets = None;
    let Some(open) = wiring.open.as_mut() else {
        return;
    };
    if &open.identity != identity || !open.is_keyed(workspace_key) {
        return;
    }
    for (key, is_stored) in writes {
        if is_stored {
            stored.insert(key);
        } else {
            stored.remove(&key);
        }
    }
    wiring.checked_secrets = Some(checked);
    // Only a change re-resolves: re-installing the inputs runs the check again, and an
    // unchanged answer has to end that loop.
    if !open.stored_secrets.replace_with(stored) {
        return;
    }
    apply(cx);
}

/// The picker's inputs changed: a confirmed choice is the new active environment; inputs
/// for another workspace (the store landing's defaults) are replaced with this one's.
fn follow_selector(cx: &mut App) {
    let Some(inputs) = cx.try_global::<SelectorInputs>() else {
        return;
    };
    let (workspace_id, active) = (inputs.workspace_id.clone(), inputs.active);
    let wiring = cx.global_mut::<ScopeWiring>();
    let Some(open) = wiring.open.as_mut() else {
        return;
    };
    if open.key.is_none() {
        return;
    }
    if !open.is_keyed(&workspace_id) {
        apply(cx);
    } else if open.active != active {
        open.active = active;
        wiring.selection_generation += 1;
        apply(cx);
    }
}

/// The environment editor refreshed its inputs after a save: take the scope's saved
/// variables and local values, re-resolve when they changed, and re-check the keychain
/// (the save may have set or removed a secret).
fn follow_env_editor(cx: &mut App) {
    let Some(inputs) = cx.try_global::<EnvEditorInputs>().cloned() else {
        return;
    };
    let wiring = cx.global_mut::<ScopeWiring>();
    let Some(open) = wiring.open.as_mut() else {
        return;
    };
    if !open.is_keyed(&inputs.workspace_id) {
        return;
    }
    let scope_id = inputs.target.scope_id().to_string();
    let mut saved_locals: Vec<LocalValue> = inputs
        .local_values
        .iter()
        .filter(|value| value.scope_id == scope_id)
        .cloned()
        .collect();
    saved_locals.sort_by(|left, right| left.key.cmp(&right.key));
    let locals_changed = open.local_values_of(&scope_id) != saved_locals;
    let Some(target) = open.target_mut(&scope_id) else {
        return;
    };
    let variables_changed = target.variables() != inputs.target.variables();
    if variables_changed || locals_changed {
        *target = inputs.target.clone();
        if locals_changed {
            open.local_values.retain(|value| value.scope_id != scope_id);
            open.local_values.extend(saved_locals);
            *wiring.locals_generations.entry(scope_id).or_default() += 1;
        }
    }
    let secrets_changed = take_secret_writes(cx);
    if variables_changed || locals_changed || secrets_changed {
        apply(cx);
    }
    check_stored_secrets(cx);
}

/// Another keychain was installed ([`SecretInputs`]): put it behind a [`TrackedKeychain`]
/// (module docs), then re-check which secrets it holds and re-resolve.
fn follow_keychain(cx: &mut App) {
    let Some(store) = secret_values::store(cx) else {
        return;
    };
    let wiring = cx.global_mut::<ScopeWiring>();
    if wiring
        .keychain
        .as_ref()
        .is_some_and(|tracked| std::ptr::addr_eq(Arc::as_ptr(tracked), Arc::as_ptr(&store)))
    {
        // The wire's own install, observed.
        return;
    }
    let tracked = Arc::new(TrackedKeychain {
        inner: store,
        writes: Mutex::default(),
    });
    wiring.keychain = Some(Arc::clone(&tracked));
    wiring.forget_secret_checks();
    let store: Arc<dyn SecretStore> = tracked;
    secret_values::install_inputs(cx, SecretInputs { store });
    apply(cx);
    check_stored_secrets(cx);
}

/// Fold the keys the app set or removed through the keychain since the last call into
/// the open workspace's stored secrets; whether that changed them.
fn take_secret_writes(cx: &mut App) -> bool {
    let wiring = cx.global::<ScopeWiring>();
    let Some(tracked) = wiring.keychain.clone().filter(|_| wiring.open.is_some()) else {
        return false;
    };
    let writes = tracked.take_writes();
    if writes.is_empty() {
        return false;
    }
    let wiring = cx.global_mut::<ScopeWiring>();
    if wiring.checking_secrets.is_some() {
        wiring.writes_during_check.extend(writes.iter().cloned());
    }
    let Some(open) = wiring.open.as_ref() else {
        return false;
    };
    open.stored_secrets.apply_writes(writes)
}

/// The live request editors, dropping dead handles.
fn live_editors(cx: &mut App) -> Vec<Entity<RequestEditor>> {
    let wiring = cx.global_mut::<ScopeWiring>();
    wiring
        .editors
        .retain(|wired| wired.editor.upgrade().is_some());
    wiring
        .editors
        .iter()
        .filter_map(|wired| wired.editor.upgrade())
        .collect()
}

/// Push the open workspace's scopes everywhere: the picker, the quick-look, the
/// environment editor and every live request editor of this workspace. An editor not
/// bound to it gets none (module docs).
fn apply(cx: &mut App) {
    take_secret_writes(cx);
    let Some(open) = cx.global::<ScopeWiring>().open.clone() else {
        return;
    };
    install_inputs(&open, cx);
    let editors = live_editors(cx);
    let drafts: HashMap<EntityId, DraftBinding> = editors
        .iter()
        .map(|editor| {
            (
                editor.entity_id(),
                DraftBinding::of(editor.read(cx).draft()),
            )
        })
        .collect();
    let bound: HashSet<EntityId> = cx
        .global_mut::<ScopeWiring>()
        .editors
        .iter_mut()
        .filter_map(|wired| {
            let entity_id = wired.editor.entity_id();
            let draft = drafts.get(&entity_id)?;
            wired.bind(draft, &open).then_some(entity_id)
        })
        .collect();
    for editor in editors {
        let of_open_workspace = bound.contains(&editor.entity_id());
        let (scopes, sources) = if of_open_workspace {
            let request_id = editor.read(cx).draft().id;
            scopes_for(&open, request_id, cx)
        } else {
            (ScopeStack::new(), SecretSources::default())
        };
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes, cx);
            editor.set_secret_sources(sources, cx);
        });
    }
}

/// Install the environment picker's, quick-look's and environment editor's inputs for
/// `open` — only once its key is known and the local store they save through has landed.
fn install_inputs(open: &OpenScopes, cx: &mut App) {
    let Some(workspace_key) = open.key.clone() else {
        return;
    };
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        return;
    };
    selector::install_inputs(
        cx,
        SelectorInputs {
            workspace_id: workspace_key.clone(),
            environments: open
                .environments
                .iter()
                .map(|(environment, _)| environment.clone())
                .collect(),
            active: open
                .active_environment()
                .map(|(environment, _)| environment.id),
            store: Arc::clone(&store),
        },
    );
    if let Some(secret_store) = secret_values::store(cx) {
        quick_look::install_inputs(
            cx,
            QuickLookInputs {
                workspace_id: workspace_key.clone(),
                environment: open
                    .active_environment()
                    .map(|(environment, _)| environment.clone()),
                globals: open
                    .globals
                    .as_ref()
                    .map(|globals| globals.variables().to_vec())
                    .unwrap_or_default(),
                local_values: open.local_values.clone(),
                secret_store,
            },
        );
    }
    let target = open
        .active_environment()
        .map(|(_, target)| target)
        .or(open.globals.as_ref());
    if let Some(target) = target {
        env_editor::install_inputs(
            cx,
            EnvEditorInputs {
                workspace_id: workspace_key,
                workspace_root: open.identity.root.clone(),
                target: target.clone(),
                local_values: open.local_values_of(target.scope_id()),
                store,
            },
        );
    }
}

/// The scopes one request resolves against — globals, its collection, the active
/// environment — and where its secrets come from (module docs).
fn scopes_for(open: &OpenScopes, request_id: ItemId, cx: &App) -> (ScopeStack, SecretSources) {
    let mut layers: Vec<(ScopeKind, &EditTarget)> = Vec::new();
    if let Some(globals) = &open.globals {
        layers.push((ScopeKind::Global, globals));
    }
    if let Some(collection) = open
        .collection_of
        .get(&request_id)
        .and_then(|id| open.collections.get(id))
    {
        layers.push((ScopeKind::Collection, collection));
    }
    if let Some((_, environment)) = open.active_environment() {
        layers.push((ScopeKind::Environment, environment));
    }
    let mut scopes = ScopeStack::new();
    let mut scope_ids = BTreeMap::new();
    let mut names = BTreeSet::new();
    for (kind, target) in layers {
        scopes.push(kind, target.variables().to_vec());
        scope_ids.insert(kind, target.scope_id().to_string());
        names.extend(
            target
                .variables()
                .iter()
                .map(|variable| variable.key.clone()),
        );
    }
    let mut local_values = Vec::new();
    for name in names {
        let Some(binding) = scopes.find(&name) else {
            continue;
        };
        if binding.variable.kind == VariableKind::Secret {
            continue;
        }
        let Some(scope_id) = scope_kind_of(&scopes, &name).and_then(|kind| scope_ids.get(&kind))
        else {
            continue;
        };
        if let Some(local) = open
            .local_values
            .iter()
            .find(|local| &local.scope_id == scope_id && local.key == name)
        {
            local_values.push((name, local.value.clone()));
        }
    }
    for (name, value) in local_values {
        scopes.set_local_value(&name, &value);
    }
    let sources = match (secret_values::store(cx), open.key.clone()) {
        (Some(store), Some(workspace_key)) => {
            // Read at call time, not captured as a snapshot (S43x): the closure outlives
            // the wiring that built it — the `UnresolvedSecret` panel re-checks a failed
            // send's own inputs after a save has added keys.
            let stored = open.stored_secrets.clone();
            SecretSources {
                store,
                key_of: Arc::new(move |kind, name| {
                    let scope_id = scope_ids.get(&kind)?;
                    secret_key(workspace_key.as_str(), scope_id.as_str(), name)
                        .ok()
                        .filter(|key| stored.contains(key))
                }),
            }
        }
        _ => SecretSources::default(),
    };
    (scopes, sources)
}
