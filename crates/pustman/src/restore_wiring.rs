//! The restore and autosave wire (S23w): every [`Workspace`] window that opens gets the
//! tabs and layout `workspace::Restore` saved under [`WORKSPACE_KEY`] back, and
//! `workspace::Autosave` keeps dirty drafts saved on its 2 s tick (PRD REQ-UI-01/02,
//! REQ-NFR-05). The snapshot and save logic lives in `workspace`; this module only wires
//! it to the things `workspace` cannot know: when a window opens, when the app's local
//! store ([`AppStore`]) lands — usually after the window, because it opens on a
//! background task (S22e) — and which item kinds rebuild the saved tabs
//! ([`register_item_kind`]). Both orders are covered: a store that is already here is
//! picked up when the workspace is created, and one that arrives later by the
//! [`AppStore`] observer. A store that never arrives ([`AppStore`] is never installed —
//! the user was already told) means no wiring at all.
//!
//! The handles [`Restore::attach`] and [`Autosave::start`] return are owned here, in the
//! app-wide [`RestoreWiring`] global, for as long as the workspace lives: dropping them
//! stops the restore and the autosave (their contract), so they are dropped when the
//! workspace is released with its window, and not before.

use std::sync::Arc;

use gpui::{App, Entity, EntityId, Global, Subscription, WeakEntity};
use workspace::{Autosave, ItemFactory, Restore, Workspace};

use crate::AppStore;

/// The key the one window of v1 saves under. Decision: v1 has a single workspace, so a
/// stable constant is its id; a multi-workspace build would derive this per window
/// instead.
const WORKSPACE_KEY: &str = "default";

/// The app-wide wiring state: the registered item kinds, the windows waiting for the
/// store, and the handles of the windows already wired.
#[derive(Default)]
struct RestoreWiring {
    started: bool,
    kinds: Vec<ItemFactory>,
    pending: Vec<WeakEntity<Workspace>>,
    windows: Vec<WiredWindow>,
    _subscriptions: Vec<Subscription>,
}

/// The restore and autosave handles of one wired window, held for as long as its
/// workspace lives (see the module docs).
struct WiredWindow {
    workspace: EntityId,
    _restore: Entity<Restore>,
    _autosave: Entity<Autosave>,
}

impl Global for RestoreWiring {}

/// Register a kind of persistent item the app can reopen (PRD REQ-UI-02): when a saved
/// tab is restored, the kinds are asked in turn with the stored item id and draft, and
/// the first one that builds an item wins. A tab no kind claims is reported in a toast
/// and left out — `workspace::Restore` owns that behavior.
///
/// The kinds are read when a tab is rebuilt, so registering after a window wired still
/// reaches that window's restore. Feature crates cannot call this themselves (they may
/// not depend on this shell crate), so [`init`] registers the kinds the app ships and
/// tests register their own here.
pub fn register_item_kind(cx: &mut App, kind: ItemFactory) {
    cx.default_global::<RestoreWiring>().kinds.push(kind);
}

/// Wire restore and autosave into the app: every [`Workspace`] created from now on gets
/// its saved tabs and layout back once the local store is there, and keeps them saved.
/// Called from [`crate::init_with_store`]; calling it again does nothing, so two
/// simulated launches (two init runs over one shared store) wire once.
pub fn init(cx: &mut App) {
    if cx.default_global::<RestoreWiring>().started {
        return;
    }
    cx.default_global::<RestoreWiring>().started = true;
    // The kinds the app's feature crates rebuild tabs from (PRD REQ-UI-02): without the
    // request editor's, every saved request tab comes back as the missing-items toast
    // (issue #103). Registered here because `request_editor` cannot depend on this crate
    // to call [`register_item_kind`] itself.
    register_item_kind(cx, request_editor::restore_item_kind());
    let subscriptions = vec![
        // "Both exist" is watched from both sides, so neither order loses a window: this
        // observer fires when the store lands after the window, and
        // `wire_pending_windows`'s store check covers the store that landed first.
        cx.observe_global::<AppStore>(wire_pending_windows),
        cx.observe_new::<Workspace>(|_workspace, _window, cx| {
            let workspace = cx.entity();
            cx.default_global::<RestoreWiring>()
                .pending
                .push(workspace.downgrade());
            // Deferred: the window is on the update stack while its workspace is being
            // created, and the wiring needs it free to reach into the window.
            cx.defer(wire_pending_windows);
        }),
    ];
    cx.default_global::<RestoreWiring>()
        ._subscriptions
        .extend(subscriptions);
}

/// Attach [`Restore`] and [`Autosave`] to every workspace waiting for a store to save
/// into, and remember their handles. Idempotent: a workspace is wired at most once,
/// whichever of the two watchers gets there first.
fn wire_pending_windows(cx: &mut App) {
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        // No store yet — or none at all, because `StoreStartup::Unavailable` never
        // installs one and the user was already told. Either way there is nothing to
        // save into: wait for `AppStore`, or for the store that never comes.
        return;
    };
    let workspaces = std::mem::take(&mut cx.default_global::<RestoreWiring>().pending);
    for workspace in workspaces {
        let Some(workspace) = workspace.upgrade() else {
            // Gone with its window before the store landed.
            continue;
        };
        let workspace_id = workspace.entity_id();
        if cx
            .default_global::<RestoreWiring>()
            .windows
            .iter()
            .any(|window| window.workspace == workspace_id)
        {
            continue;
        }
        let Some((restore, autosave)) = cx.with_window(workspace_id, |window, cx| {
            let restore = Restore::attach(
                &workspace,
                Arc::clone(&store),
                WORKSPACE_KEY.to_owned(),
                item_factory(),
                window,
                cx,
            );
            let autosave = Autosave::start(&restore, &workspace, cx);
            (restore, autosave)
        }) else {
            // The window closed before there was a store to save into.
            continue;
        };
        let release = cx.observe_release(&workspace, move |_workspace, cx| {
            // The handles live as long as the window: this is where they go.
            cx.default_global::<RestoreWiring>()
                .windows
                .retain(|window| window.workspace != workspace_id);
        });
        cx.default_global::<RestoreWiring>()
            ._subscriptions
            .push(release);
        cx.default_global::<RestoreWiring>()
            .windows
            .push(WiredWindow {
                workspace: workspace_id,
                _restore: restore,
                _autosave: autosave,
            });
    }
}

/// The factory [`Restore`] rebuilds tabs through: every registered kind is asked in
/// turn, and a tab none of them claims comes back `None` — "the backing item is gone"
/// (REQ-UI-02), which [`init`]'s registration keeps from happening to the app's own
/// tabs.
fn item_factory() -> ItemFactory {
    Arc::new(|item_id, draft, window, cx| {
        let kinds = cx.default_global::<RestoreWiring>().kinds.clone();
        kinds
            .into_iter()
            .find_map(|kind| kind(item_id, draft.clone(), window, cx))
    })
}
