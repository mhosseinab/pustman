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
//! `Decision:` which secrets the keychain holds is checked off the foreground whenever
//! the scopes or the keychain change, and [`SecretSources`] names a key only for those.
//! The pre-send check (#198) then warns about a secret with nothing stored — and
//! "Send anyway" fails it as unresolved — without touching the keychain on the
//! foreground. A keychain that cannot be reached counts as holding the entry, so the
//! send reports that failure itself.
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
//! `Decision:` every request editor is bound to the workspace open when it was built (or
//! the first one opened after). An editor from another workspace — a tab left open from
//! folder A after folder B opens — gets no scopes and no keychain, so its secrets stay
//! unresolved and the pre-send check warns, rather than resolving B's secrets into a
//! request to A's host.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, VariableKind};
use environment_picker::env_editor::{self, EditTarget, EnvEditorInputs};
use environment_picker::quick_look::{self, QuickLookInputs};
use environment_picker::secret_values::{self, SecretInputs};
use environment_picker::selector::{self, SelectorInputs};
use gpui::{App, Entity, Global, Subscription, Task, WeakEntity};
use request_editor::{RequestEditor, SecretSources, scope_kind_of};
use secrets::{KeyringStore, SecretError, SecretKey, SecretStore};
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

/// One opened workspace as the wire tells them apart: its `pustman.json` id and the
/// folder it was opened from. Known as soon as the snapshot is.
#[derive(Clone, PartialEq)]
struct WorkspaceIdentity {
    id: String,
    root: PathBuf,
}

/// One request editor and the workspace it belongs to (module docs); `None` until a
/// workspace is open.
struct WiredEditor {
    editor: WeakEntity<RequestEditor>,
    workspace: Option<WorkspaceIdentity>,
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
    /// Bumped by every selection the user makes: a stored-state read started before one
    /// does not overwrite it.
    selection_generation: u64,
    /// Bumped by every environment-editor save that changed local values: a stored-state
    /// read started before one does not overwrite them.
    locals_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl Global for ScopeWiring {}

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
    local_values: Vec<LocalValue>,
    active: Option<ItemId>,
    /// The secret keys the keychain holds an entry for (module docs).
    stored_secrets: Arc<HashSet<SecretKey>>,
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
/// The folder is hex-encoded rather than hashed: the encoding is injective, stable
/// across toolchains, and carries no `/` (a [`SecretKey`] id must not).
pub fn workspace_key(workspace_id: &str, canonical_root: &Path) -> String {
    format!(
        "{workspace_id}.{}",
        hex::encode(canonical_root.as_os_str().as_encoded_bytes())
    )
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
            let open = cx.global::<ScopeWiring>().open.clone();
            cx.global_mut::<ScopeWiring>().editors.push(WiredEditor {
                editor: entity.downgrade(),
                workspace: open.as_ref().map(|open| open.identity.clone()),
            });
            let Some(open) = open else {
                return;
            };
            let (scopes, sources) = scopes_for(&open, editor.draft().id, cx);
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
        cx.observe_global::<SecretInputs>(|cx| {
            apply(cx);
            check_stored_secrets(cx);
        }),
    ];
    cx.global_mut::<ScopeWiring>()
        ._subscriptions
        .extend(subscriptions);
}

/// Follow `workspace`'s open folder: every open and snapshot change re-reads its scopes.
fn watch_workspace(workspace: &Entity<Workspace>, cx: &mut App) {
    let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
        StartupProblems::push(cx, WIRE_FAILURE_MESSAGE);
        return;
    };
    let subscription = cx.subscribe(&opener, |opener, _event: &WorkspaceOpenEvent, cx| {
        open_changed(&opener, cx);
    });
    cx.global_mut::<ScopeWiring>()
        ._subscriptions
        .push(subscription);
    open_changed(&opener, cx);
}

/// The open workspace or its snapshot changed: rebuild the scopes from the snapshot. A
/// different workspace starts with no key, no active environment and no local values
/// until its own are read; the same workspace keeps all three.
fn open_changed(opener: &Entity<WorkspaceOpen>, cx: &mut App) {
    let Some(snapshot) = opener.read(cx).snapshot() else {
        return;
    };
    let Some(mut scopes) = scopes_of(snapshot) else {
        StartupProblems::push(cx, WIRE_FAILURE_MESSAGE);
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
                StartupProblems::push(cx, GLOBALS_FAILURE_MESSAGE);
            }
            remove_inputs(cx);
            let wiring = cx.global_mut::<ScopeWiring>();
            wiring.open = Some(scopes);
            wiring.checking = None;
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
    wiring.checking = None;
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
        local_values: Vec::new(),
        active: None,
        stored_secrets: Arc::default(),
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
    let (selection_generation, locals_generation) =
        (wiring.selection_generation, wiring.locals_generation);
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
            let take_locals = wiring.locals_generation == locals_generation;
            let Some(open) = wiring.open.as_mut() else {
                return;
            };
            if open.identity != identity || !open.is_keyed(&key) {
                return;
            }
            match state {
                Ok((local_values, selection)) => {
                    // A choice or a save the user made while this read was in flight is
                    // newer than what it read: that half of the read is discarded.
                    if take_locals {
                        open.local_values = local_values;
                    }
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
                    StartupProblems::push(cx, LOAD_FAILURE_MESSAGE);
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
            StartupProblems::push(cx, FOLDER_FAILURE_MESSAGE);
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
/// keychain holds an entry for, and re-resolve when that changed (module docs). Only
/// whether an entry exists is kept; every value read is dropped on the spot.
fn check_stored_secrets(cx: &mut App) {
    let Some(open) = cx.global::<ScopeWiring>().open.as_ref() else {
        return;
    };
    let Some(workspace_key) = open.key.clone() else {
        return;
    };
    let identity = open.identity.clone();
    let mut keys = Vec::new();
    for target in open.targets() {
        for variable in target.variables() {
            if variable.kind != VariableKind::Secret {
                continue;
            }
            if let Ok(key) = SecretKey::new(
                workspace_key.as_str(),
                target.scope_id(),
                variable.key.as_str(),
            ) {
                keys.push(key);
            }
        }
    }
    let Some(store) = secret_values::store(cx) else {
        return;
    };
    let task = cx.spawn(async move |cx| {
        let stored = cx
            .background_executor()
            .spawn(async move {
                keys.into_iter()
                    .filter(|key| !matches!(store.get(key), Err(SecretError::NotFound { .. })))
                    .collect::<HashSet<SecretKey>>()
            })
            .await;
        cx.update(|cx| {
            let Some(open) = cx.global_mut::<ScopeWiring>().open.as_mut() else {
                return;
            };
            // Only a change re-resolves: re-installing the inputs runs this check again,
            // and an unchanged answer has to end that loop.
            if open.identity != identity
                || !open.is_keyed(&workspace_key)
                || *open.stored_secrets == stored
            {
                return;
            }
            open.stored_secrets = Arc::new(stored);
            apply(cx);
        });
    });
    cx.global_mut::<ScopeWiring>().checking = Some(task);
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
            wiring.locals_generation += 1;
        }
        apply(cx);
    }
    check_stored_secrets(cx);
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
/// environment editor and every live request editor of this workspace. An editor of
/// another workspace gets none (module docs).
fn apply(cx: &mut App) {
    let Some(open) = cx.global::<ScopeWiring>().open.clone() else {
        return;
    };
    install_inputs(&open, cx);
    let editors: Vec<(Entity<RequestEditor>, bool)> = {
        let wiring = cx.global_mut::<ScopeWiring>();
        wiring
            .editors
            .retain(|wired| wired.editor.upgrade().is_some());
        wiring
            .editors
            .iter_mut()
            .filter_map(|wired| {
                let editor = wired.editor.upgrade()?;
                // An editor built before any workspace opened belongs to the first one.
                let workspace = wired.workspace.get_or_insert_with(|| open.identity.clone());
                Some((editor, *workspace == open.identity))
            })
            .collect()
    };
    for (editor, of_open_workspace) in editors {
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
            let stored = Arc::clone(&open.stored_secrets);
            SecretSources {
                store,
                key_of: Arc::new(move |kind, name| {
                    let scope_id = scope_ids.get(&kind)?;
                    SecretKey::new(workspace_key.as_str(), scope_id.as_str(), name)
                        .ok()
                        .filter(|key| stored.contains(key))
                }),
            }
        }
        _ => SecretSources::default(),
    };
    (scopes, sources)
}
