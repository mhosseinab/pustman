//! The variable-scope wire (S43w): the opened workspace's globals, its active environment
//! and each request's collection become the scopes every open [`RequestEditor`] resolves
//! `{{var}}` against (REQ-VAR-02/08), and the OS keychain becomes where its secret-kind
//! variables resolve from at send (REQ-VAR-SHARE-02). The same state feeds the
//! environment picker, the quick-look and the environment editor, so all four see one
//! workspace and one active environment.
//!
//! What it follows, and what each change re-resolves:
//! - [`WorkspaceOpen`]'s events: a workspace opening (its local values and persisted
//!   active environment are then read off the foreground) or its snapshot changing.
//! - [`SelectorInputs`]: the picker's confirmed choice is the active environment.
//! - [`EnvEditorInputs`]: the environment editor refreshes them after a save, which is
//!   how a changed shared or local value reaches the open editors.
//! - [`SecretInputs`]: the keychain every editor's [`SecretSources`] reads at send.
//!
//! `Decision:` the scope rules are `variables`' own: the stack is built with
//! [`ScopeStack::push`], and a local value is applied to a name only when its winning
//! variable (the one [`scope_kind_of`] names) lives in the scope that local value was
//! saved for — local values are per scope in storage but per name in the stack. A secret
//! never takes a value from storage: its only value is the keychain's, read by
//! `http_engine::prepare` at send through [`SecretSources`]. Nothing here reads a secret
//! value, so the pre-send check (#198) sees a keyed secret as resolved without touching
//! the keychain on the foreground.
//!
//! `Decision:` the workspace id that keys local values, keychain entries and the active
//! environment's selection is the opened workspace's own `pustman.json` id, not
//! [`crate::WORKSPACE_ID`]: two workspace folders must never share a secret or a
//! selection. History, cookies and restored tabs keep `WORKSPACE_ID`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use api_model::{Environment, ItemId, VariableKind};
use environment_picker::env_editor::{self, EditTarget, EnvEditorInputs};
use environment_picker::quick_look::{self, QuickLookInputs};
use environment_picker::secret_values::{self, SecretInputs};
use environment_picker::selector::{self, SelectorInputs};
use gpui::{App, AppContext, Entity, Global, Subscription, Task, WeakEntity};
use request_editor::{RequestEditor, SecretSources, scope_kind_of};
use secrets::{KeyringStore, SecretKey, SecretStore};
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

/// The app-wide wire: the editors it pushes scopes into and the open workspace's scopes.
#[derive(Default)]
struct ScopeWiring {
    started: bool,
    /// Every request editor built since `init`; dead handles are dropped on each push.
    editors: Vec<WeakEntity<RequestEditor>>,
    open: Option<OpenScopes>,
    /// The read of the open workspace's local values in flight: a newer open replaces —
    /// cancels — it, so an older workspace's values can never land on a newer one.
    loading: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Global for ScopeWiring {}

/// One opened workspace's scopes. [`EditTarget`] names each scope's id and variables —
/// the same ids the environment editor saves local values and keychain entries under.
#[derive(Clone)]
struct OpenScopes {
    workspace_id: String,
    root: PathBuf,
    /// `None` when the workspace has no readable globals list.
    globals: Option<EditTarget>,
    /// The environments in the picker's order (name, then id), each with its scope.
    environments: Vec<(Environment, EditTarget)>,
    collections: BTreeMap<ItemId, EditTarget>,
    /// Which collection each request belongs to, through any folders.
    collection_of: BTreeMap<ItemId, ItemId>,
    local_values: Vec<LocalValue>,
    active: Option<ItemId>,
}

impl OpenScopes {
    fn active_environment(&self) -> Option<&(Environment, EditTarget)> {
        let active = self.active?;
        self.environments
            .iter()
            .find(|(environment, _)| environment.id == active)
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
            cx.global_mut::<ScopeWiring>()
                .editors
                .push(entity.downgrade());
            let Some(open) = cx.global::<ScopeWiring>().open.clone() else {
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
        cx.observe_global::<SecretInputs>(apply),
    ];
    cx.global_mut::<ScopeWiring>()
        ._subscriptions
        .extend(subscriptions);
}

/// Follow `workspace`'s open folder: every open and snapshot change re-reads its scopes.
fn watch_workspace(workspace: &Entity<Workspace>, cx: &mut App) {
    let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
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
/// different workspace starts with no active environment and no local values until its
/// stored ones are read; the same workspace keeps both.
fn open_changed(opener: &Entity<WorkspaceOpen>, cx: &mut App) {
    let Some(snapshot) = opener.read(cx).snapshot() else {
        return;
    };
    let Some(mut scopes) = scopes_of(snapshot) else {
        return;
    };
    if scopes.globals.is_none() && has_globals_field(snapshot) {
        StartupProblems::push(cx, GLOBALS_FAILURE_MESSAGE);
    }
    let previous = cx.global_mut::<ScopeWiring>().open.take();
    let same_workspace = previous.as_ref().is_some_and(|previous| {
        previous.workspace_id == scopes.workspace_id && previous.root == scopes.root
    });
    if let Some(previous) = previous
        && same_workspace
    {
        scopes.local_values = previous.local_values;
        scopes.active = previous.active;
    }
    cx.global_mut::<ScopeWiring>().open = Some(scopes);
    if same_workspace {
        apply(cx);
    } else {
        load_open_workspace(cx);
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
        workspace_id,
        root: snapshot.root.clone(),
        globals,
        environments,
        collections,
        collection_of: collection_of(snapshot),
        local_values: Vec::new(),
        active: None,
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

/// Read the open workspace's local values and stored active environment off the
/// foreground, then push the scopes. Without a store yet, push what there is; the store
/// landing runs this again.
fn load_open_workspace(cx: &mut App) {
    let Some(workspace_id) = cx
        .global::<ScopeWiring>()
        .open
        .as_ref()
        .map(|open| open.workspace_id.clone())
    else {
        return;
    };
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        apply(cx);
        return;
    };
    let read = cx.background_spawn({
        let workspace_id = workspace_id.clone();
        async move { read_workspace_state(&store, &workspace_id) }
    });
    let task = cx.spawn(async move |cx| {
        let state = read.await;
        cx.update(|cx| {
            let wiring = cx.global_mut::<ScopeWiring>();
            let Some(open) = wiring.open.as_mut() else {
                return;
            };
            if open.workspace_id != workspace_id {
                return;
            }
            match state {
                Ok((local_values, selection)) => {
                    open.local_values = local_values;
                    open.active = selection.and_then(|selected| {
                        open.environments
                            .iter()
                            .map(|(environment, _)| environment.id)
                            .find(|id| id.to_string() == selected)
                    });
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

/// The workspace's non-secret local values and its stored active environment id.
fn read_workspace_state(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
) -> anyhow::Result<(Vec<LocalValue>, Option<String>)> {
    let store = store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))?;
    let local_values = store.local_values().list(workspace_id)?;
    let key = selector::selection_key(workspace_id);
    let selection = store
        .kv()
        .list()?
        .into_iter()
        .find(|row| row.key == key)
        .map(|row| row.value);
    Ok((local_values, selection))
}

/// The picker's inputs changed: a confirmed choice is the new active environment; inputs
/// for another workspace (the store landing's defaults) are replaced with this one's.
fn follow_selector(cx: &mut App) {
    let Some(inputs) = cx.try_global::<SelectorInputs>() else {
        return;
    };
    let (workspace_id, active) = (inputs.workspace_id.clone(), inputs.active);
    let Some(open) = cx.global_mut::<ScopeWiring>().open.as_mut() else {
        return;
    };
    if open.workspace_id != workspace_id {
        apply(cx);
    } else if open.active != active {
        open.active = active;
        apply(cx);
    }
}

/// The environment editor refreshed its inputs after a save: take the scope's saved
/// variables and local values, and re-resolve when they changed.
fn follow_env_editor(cx: &mut App) {
    let Some(inputs) = cx.try_global::<EnvEditorInputs>().cloned() else {
        return;
    };
    let Some(open) = cx.global_mut::<ScopeWiring>().open.as_mut() else {
        return;
    };
    if open.workspace_id != inputs.workspace_id {
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
    if !variables_changed && !locals_changed {
        return;
    }
    *target = inputs.target.clone();
    if locals_changed {
        open.local_values.retain(|value| value.scope_id != scope_id);
        open.local_values.extend(saved_locals);
    }
    apply(cx);
}

/// Push the open workspace's scopes everywhere: the picker, the quick-look, the
/// environment editor and every live request editor.
fn apply(cx: &mut App) {
    let Some(open) = cx.global::<ScopeWiring>().open.clone() else {
        return;
    };
    install_inputs(&open, cx);
    let editors: Vec<Entity<RequestEditor>> = {
        let wiring = cx.global_mut::<ScopeWiring>();
        wiring.editors.retain(|editor| editor.upgrade().is_some());
        wiring
            .editors
            .iter()
            .filter_map(WeakEntity::upgrade)
            .collect()
    };
    for editor in editors {
        let request_id = editor.read(cx).draft().id;
        let (scopes, sources) = scopes_for(&open, request_id, cx);
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes, cx);
            editor.set_secret_sources(sources, cx);
        });
    }
}

/// Install the environment picker's, quick-look's and environment editor's inputs for
/// `open` — only once the local store they save through has landed.
fn install_inputs(open: &OpenScopes, cx: &mut App) {
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        return;
    };
    selector::install_inputs(
        cx,
        SelectorInputs {
            workspace_id: open.workspace_id.clone(),
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
                workspace_id: open.workspace_id.clone(),
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
                workspace_id: open.workspace_id.clone(),
                workspace_root: open.root.clone(),
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
    let sources = match secret_values::store(cx) {
        Some(store) => {
            let workspace_id = open.workspace_id.clone();
            SecretSources {
                store,
                key_of: Arc::new(move |kind, name| {
                    let scope_id = scope_ids.get(&kind)?;
                    SecretKey::new(workspace_id.as_str(), scope_id.as_str(), name).ok()
                }),
            }
        }
        None => SecretSources::default(),
    };
    (scopes, sources)
}
