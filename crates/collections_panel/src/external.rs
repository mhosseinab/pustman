//! The collections tree's external-change feed (REQ-COLLAB-01): when a window opens a
//! workspace, [`workspace_store::watch`] starts over its folder and every external
//! change — a `git pull`, a branch switch, a teammate's edit on a shared drive — lands
//! in the snapshot the tree draws within the 1 s budget `workspace_store`'s own watcher
//! tests pin. The tree renders whatever [`WorkspaceOpen`] holds, so the tree, the open
//! tabs and the recent list all read one snapshot: the opened workspace's.
//!
//! `Decision:` a batch of [`FsChange`]s is applied by one fresh [`workspace_store::open`]
//! re-read rather than per-event patching: every variant of [`FsChange`] says "disk
//! differs from the snapshot", `open` is the crate's only read door and rebuilds the
//! snapshot wholesale (problems included), and one read per batch cannot half-apply a
//! batch's own inconsistencies.
//!
//! `Decision:` the watcher's receiver is drained on a [`CHANGE_POLL`] GPUI timer instead
//! of a bridging thread: a wake from a foreign thread is non-determinism the test
//! scheduler refuses unless parking is allowed (the S41a git-status `Decision:`), and a
//! 100 ms poll keeps the path inside the 1 s budget (100 ms debounce + 100 ms poll + one
//! read). The S42a test passes real time through `allow_parking`, the seam for "a mix of
//! deterministic and non-deterministic async behavior, such as when interacting with
//! I/O".
//!
//! `Decision:` the feed replaces the tree's snapshot directly rather than through S40a's
//! per-tree write queue: `run_write` takes the window an action handler owns and its
//! work re-reads the disk itself, so a queued feed would read twice per batch and still
//! not carry [`WorkspaceOpen::snapshot`]. A watcher read that began before an in-flight
//! write can therefore land after that write's snapshot and briefly show the pre-write
//! tree; it heals on the next batch (Pustman's own writes are not recorded as own
//! writes, so every write triggers one), and the window needs external changes and a
//! user write inside one millisecond-scale read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use gpui::{App, Context, Entity, EntityId, Global, Subscription, Task, WeakEntity};
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen, WorkspaceOpenEvent};
use workspace_store::{FsChange, OpenError, WatchError, WorkspaceSnapshot, WorkspaceWatcher};

use crate::tree::CollectionsTree;

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;

/// How often the watcher's receiver is drained (see the module `Decision:`): one poll
/// plus the watcher's own debounce keeps an external change well under 1 s end to end.
const CHANGE_POLL: Duration = Duration::from_millis(100);

/// The re-reads per workspace root (see [`read_workspace`]): keyed by root, so one
/// lifecycle test can count its own workspace's reads while the other tests read theirs
/// on parallel threads.
#[cfg(test)]
static DISK_READS: std::sync::Mutex<BTreeMap<PathBuf, usize>> =
    std::sync::Mutex::new(BTreeMap::new());

/// How many workspace re-reads the feeds have made for `root` — the lifecycle tests'
/// proof that a closed window's feed stopped reading (S42a fix 1).
#[cfg(test)]
pub(crate) fn disk_reads(root: &Path) -> usize {
    DISK_READS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(root)
        .copied()
        .unwrap_or(0)
}

/// How many tree feeds are live — the lifecycle tests' "the feed is gone" (S42a fix 1).
#[cfg(test)]
pub(crate) fn live_feeds(cx: &App) -> usize {
    cx.try_global::<Feeds>()
        .map_or(0, |feeds| feeds.by_tree.len())
}

/// Re-read `root`'s workspace. One door for the feed's reads, so the lifecycle tests
/// can count them (S40b's injected-failure seam's shape).
fn read_workspace(root: &Path) -> Result<WorkspaceSnapshot, OpenError> {
    #[cfg(test)]
    {
        let mut reads = DISK_READS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *reads.entry(root.to_path_buf()).or_insert(0) += 1;
    }
    workspace_store::open(root)
}

/// The app-wide wiring: the init guard and one feed per tree, kept alive for as long as
/// its tree so the watcher runs while the panel is on screen.
#[derive(Default)]
struct Feeds {
    started: bool,
    by_tree: BTreeMap<EntityId, TreeEntry>,
    /// Kept so the tree observer keeps firing (AGENTS.md).
    _subscriptions: Vec<Subscription>,
}

/// One live feed, and the subscription that drops it when its tree is released (S42a
/// fix 1): stored beside the feed it cleans up, so both leave together instead of the
/// subscription outliving its tree in a global vector.
struct TreeEntry {
    _feed: Entity<TreeFeed>,
    _release: Subscription,
}

impl Global for Feeds {}

/// Give every collections tree its external-change feed (REQ-COLLAB-01). Wired when the
/// tree appears — the panel registers deferred off the workspace's own update — and
/// paired with the [`WorkspaceOpen`] its window already has (`workspace::init` attaches
/// one alongside the workspace).
pub fn init(cx: &mut App) {
    if cx.default_global::<Feeds>().started {
        return;
    }
    cx.default_global::<Feeds>().started = true;
    let subscription = cx.observe_new::<CollectionsTree>(|_tree, _window, cx| {
        let tree = cx.entity();
        // Deferred like the panel's own registration: the wiring runs inside the tree's
        // creation update and its window lookups need that stack clear.
        cx.defer(move |cx| wire_tree(&tree, cx));
    });
    cx.default_global::<Feeds>()
        ._subscriptions
        .push(subscription);
}

/// Pair `tree` with its window's [`WorkspaceOpen`] and start feeding it. Idempotent:
/// one feed per tree, whichever callback gets here first.
fn wire_tree(tree: &Entity<CollectionsTree>, cx: &mut App) {
    let tree_id = tree.entity_id();
    if cx.default_global::<Feeds>().by_tree.contains_key(&tree_id) {
        return;
    }
    let opener = cx
        .with_window(tree_id, |window, cx| {
            window
                .root::<Workspace>()
                .flatten()
                .and_then(|workspace| WorkspaceOpen::for_window(workspace.entity_id(), cx))
        })
        .flatten();
    let Some(opener) = opener else {
        // Handled: this tree has no `WorkspaceOpen` — a test view wired without
        // `workspace::init`, or the window closed before this deferred wiring ran.
        // There is no workspace folder to watch.
        return;
    };
    let feed = cx.new(|cx| TreeFeed::new(tree.downgrade(), opener, cx));
    let release = cx.observe_release(tree, move |_tree, cx| {
        // The tree goes with its window, so the feed goes with it — its watcher, its
        // pump and its subscription stop instead of running for nobody (S42a fix 1).
        // Removing the entry also drops the release subscription firing here; the
        // subscriber set has already retired it, so that drop is a no-op.
        cx.default_global::<Feeds>().by_tree.remove(&tree_id);
    });
    cx.default_global::<Feeds>().by_tree.insert(
        tree_id,
        TreeEntry {
            _feed: feed,
            _release: release,
        },
    );
}

/// One tree's feed: the watcher over its window's open workspace folder, the pump that
/// turns [`FsChange`] batches into fresh snapshots, and the subscription that hands the
/// opened snapshot to the tree.
struct TreeFeed {
    /// Held weakly (S42a fix 1): a feed that strongly held its tree would keep the tree
    /// alive after its window closed, and the watcher would run for nobody.
    tree: WeakEntity<CollectionsTree>,
    opener: Entity<WorkspaceOpen>,
    /// The live watcher and its event receiver; dropping the pair stops the watcher.
    source: Option<(WorkspaceWatcher, Receiver<FsChange>)>,
    /// The pump: starts the watcher and refreshes the snapshot on every change batch.
    /// Replacing it cancels the old one — a dropped [`Task`] cancels its work
    /// (AGENTS.md) — which is what a newer open wants.
    pump: Option<Task<()>>,
    /// Whether a read failure has been reported for the streak in flight (S42a fix 2):
    /// one toast per streak, never one per change batch.
    read_failed: bool,
    /// The failure toast's dismissal handle, while one is showing (S42a fix 2).
    problem_toast: Option<usize>,
    /// Kept so the open events keep reaching [`Self::on_open_event`] (AGENTS.md).
    _opened: Subscription,
}

impl TreeFeed {
    /// A feed that adopts whatever `opener`'s window already has open (a launch reopen
    /// can land before this tree exists) and follows its events from here on.
    fn new(
        tree: WeakEntity<CollectionsTree>,
        opener: Entity<WorkspaceOpen>,
        cx: &mut Context<Self>,
    ) -> Self {
        let opened = cx.subscribe(&opener, Self::on_open_event);
        let mut feed = Self {
            tree,
            opener,
            source: None,
            pump: None,
            read_failed: false,
            problem_toast: None,
            _opened: opened,
        };
        feed.follow(cx);
        feed
    }

    /// The opener's events: a new open starts its folder's watcher, and either variant
    /// re-reads [`WorkspaceOpen::snapshot`] into the tree (S42a).
    fn on_open_event(
        &mut self,
        _opener: Entity<WorkspaceOpen>,
        event: &WorkspaceOpenEvent,
        cx: &mut Context<Self>,
    ) {
        if let WorkspaceOpenEvent::Opened { root } = event {
            self.watch(root.clone(), cx);
        }
        self.feed_tree(cx);
    }

    /// Adopt whatever this window has open right now (see [`Self::new`]).
    fn follow(&mut self, cx: &mut Context<Self>) {
        let root = self
            .opener
            .read_with(cx, |open, _| open.root().map(Path::to_path_buf));
        if let Some(root) = root {
            self.watch(root, cx);
        }
        self.feed_tree(cx);
    }

    /// Watch `root` for external changes, replacing whatever this feed watched before
    /// (dropping the old watcher stops its events; dropping the old pump cancels it).
    fn watch(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.source = None;
        self.pump = Some(cx.spawn(async move |this, cx| {
            'watcher: loop {
                // `watch` scans the folder before it arms the notify handle — blocking
                // IO, so it runs off the foreground thread (AGENTS.md › async).
                let started = cx
                    .background_spawn({
                        let root = root.clone();
                        async move { workspace_store::watch(&root) }
                    })
                    .await;
                match this.update(cx, |feed, cx| feed.store(started, cx)) {
                    Ok(true) => {}
                    // Either the failure is on screen already or the panel went away
                    // with its window: nothing left to pump.
                    _ => return,
                }
                // Refresh until the watcher's thread drops off, then start a new one.
                loop {
                    let snapshot = cx
                        .background_spawn({
                            let root = root.clone();
                            async move { read_workspace(&root) }
                        })
                        .await;
                    match this.update(cx, |feed, cx| feed.apply(&root, snapshot, cx)) {
                        Ok(true) => {}
                        _ => return,
                    }
                    loop {
                        cx.background_executor().timer(CHANGE_POLL).await;
                        match this.update(cx, |feed, _| feed.take_changes()) {
                            Ok(Some(changes)) if changes.is_empty() => {}
                            Ok(Some(_)) => break,
                            // The watcher's thread is gone: re-watch and re-read, or
                            // the changes in between are lost without a signal.
                            Ok(None) => continue 'watcher,
                            Err(_) => return,
                        }
                    }
                }
            }
        }));
    }

    /// Keep the started watcher, or tell the user why there is none. `false` stops the
    /// pump.
    fn store(
        &mut self,
        started: Result<(WorkspaceWatcher, Receiver<FsChange>), WatchError>,
        cx: &mut Context<Self>,
    ) -> bool {
        match started {
            Ok(source) => {
                self.source = Some(source);
                true
            }
            Err(error) => {
                self.report(
                    format!("The workspace folder could not be watched — {error}"),
                    cx,
                );
                false
            }
        }
    }

    /// The changes the watcher reported since the last poll: `Some` with an empty batch
    /// means "quiet". `None` — no watcher to drain — tells the pump to start a new one.
    /// A batch is a trigger, not a patch: see the module `Decision:`.
    fn take_changes(&mut self) -> Option<Vec<FsChange>> {
        let mut changes = Vec::new();
        let (_watcher, receiver) = self.source.as_ref()?;
        loop {
            match receiver.try_recv() {
                Ok(change) => changes.push(change),
                Err(TryRecvError::Empty) => return Some(changes),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        self.source = None;
        None
    }

    /// Put `snapshot` — the workspace as the re-read found it — in front of the window
    /// (REQ-COLLAB-01): [`WorkspaceOpen::set_snapshot`] replaces the opened snapshot and
    /// its event feeds the tree. `false` stops the pump: this window opened another
    /// workspace while the read was in flight and a newer pump owns the folder now.
    fn apply(
        &mut self,
        root: &Path,
        snapshot: Result<WorkspaceSnapshot, OpenError>,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = self
            .opener
            .read_with(cx, |open, _| open.root().map(Path::to_path_buf));
        if current.as_deref() != Some(root) {
            return false;
        }
        match snapshot {
            Ok(snapshot) => {
                if let Some(toast) = self.problem_toast.take() {
                    // The workspace reads again: the failure is over and its toast goes
                    // (S42a fix 2 — clear on the next successful read).
                    self.dismiss_toast(toast, cx);
                }
                self.read_failed = false;
                self.opener
                    .update(cx, |open, cx| open.set_snapshot(snapshot, cx));
                true
            }
            Err(error) => {
                if !self.read_failed {
                    // A workspace file a git checkout is half-way through can fail to
                    // read. One toast stands for the whole streak (S42a fix 2): a
                    // deleted or renamed root must not stack one per change batch.
                    self.read_failed = true;
                    self.problem_toast = self.report(
                        format!(
                            "Could not re-read {} after a change on disk — {error}",
                            root.display()
                        ),
                        cx,
                    );
                }
                true
            }
        }
    }

    /// Hand the tree the opened workspace's snapshot: the rows it draws are whatever
    /// [`WorkspaceOpen`] holds, however it changed (S42a).
    fn feed_tree(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self
            .opener
            .read_with(cx, |open, _| open.snapshot().map(snapshot_copy))
        else {
            return;
        };
        let landed = self
            .tree
            .update(cx, |tree, cx| tree.set_snapshot(snapshot, cx));
        if landed.is_err() {
            // Handled: the panel went away with its window and its feed goes with it
            // (S42a fix 1) — there is no tree left to feed.
        }
    }

    /// Show `message` on this window's workspace: a feed failure the user must see
    /// (AGENTS.md › errors reach the user). Returns the toast's dismissal handle, or
    /// `None` when the message could only be logged — a window that is gone has no user
    /// left (crud's `notify_user` shape).
    fn report(&self, message: impl Into<SharedString>, cx: &mut Context<Self>) -> Option<usize> {
        let message = message.into();
        let tree = self.tree.clone();
        let shown = cx.with_window(tree.entity_id(), |window, cx| {
            let Some(Some(workspace)) = window.root::<Workspace>() else {
                return None;
            };
            Some(workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message.clone(), Severity::Error, None, cx)
            }))
        });
        match shown.flatten() {
            Some(handle) => Some(handle),
            None => {
                log::error!("{message}");
                None
            }
        }
    }

    /// Take `toast` off this window's stack: the failure it reported is over (S42a fix 2).
    fn dismiss_toast(&self, toast: usize, cx: &mut Context<Self>) {
        let tree = self.tree.clone();
        let dismissed = cx.with_window(tree.entity_id(), |window, cx| {
            let Some(Some(workspace)) = window.root::<Workspace>() else {
                return false;
            };
            workspace.update(cx, |workspace, cx| {
                workspace.dismiss_toast(toast, cx);
            });
            true
        });
        if !matches!(dismissed, Some(true)) {
            // Handled: the window is gone and took its toasts with it.
        }
    }
}

/// [`WorkspaceSnapshot`] is not `Clone` (its crate owns its own types), so the tree's
/// copy of the opened workspace's snapshot is rebuilt field by field — the items and
/// problems inside it are `Clone`.
fn snapshot_copy(snapshot: &WorkspaceSnapshot) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        root: snapshot.root.clone(),
        items: snapshot.items.clone(),
        problems: snapshot.problems.clone(),
        read_only: snapshot.read_only,
    }
}
