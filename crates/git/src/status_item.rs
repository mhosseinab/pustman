//! The status bar's branch item (REQ-COLLAB-03): inside a Git repository the strip shows
//! the checked-out branch and how much is dirty in the workspace's working tree. It hides
//! outside a repository, refreshes when the workspace's watcher reports a change (this
//! window's [`WorkspaceOpen`] events) and whenever its window regains focus — the moment
//! a branch switched in a terminal becomes visible here — and a repository it cannot read
//! is shown as an inline error, never hidden (the S44a carry-forward: `git::status` errs
//! for real failures and those must reach the user). Every git call runs on a background
//! task (AGENTS.md › async).
//!
//! `Decision:` this crate wires its own item ([`init`]) instead of the app shell's
//! `status_items`: the step's Edits keep the shell out of it, and the shape —
//! `observe_new::<Workspace>`, deferred, one `add_left_item` per window — is the one the
//! shell's readouts already use.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use gpui::{App, Context, Entity, EntityId, Global, Subscription, Task, WeakEntity, Window};
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen};

use crate::status::{GitError, GitStatus};

/// A `git` answer on its way to the item. Boxed so the app can answer with a real `git`
/// child ([`crate::status`]) and the tests with a fixed value: a `smol::process` future is
/// woken by the async-io reactor's thread, non-determinism GPUI's test scheduler refuses
/// (the S41a Decision this seam exists for).
type StatusAnswer = Pin<Box<dyn Future<Output = Result<Option<GitStatus>, GitError>> + Send>>;

/// Where the item gets git's answer from.
trait StatusSource: Send + Sync {
    /// The branch and dirty count of the repository containing `root` — `Ok(None)` when
    /// `root` is not inside a repository.
    fn status(&self, root: PathBuf) -> StatusAnswer;
}

/// The app's source: the user's own `git` CLI ([`crate::status`]).
struct GitCli;

impl StatusSource for GitCli {
    fn status(&self, root: PathBuf) -> StatusAnswer {
        Box::pin(async move {
            // `crate::status`'s future is not `Send` — it holds the environment iterator
            // across its await — so it is driven to completion here, on the background
            // executor's thread (AGENTS.md › async). The child's IO is the async-io
            // reactor's work; this thread only waits for it.
            smol::block_on(crate::status(&root))
        })
    }
}

/// The debug selector on the item's rendered element — the one string tests hand to
/// `VisualTestContext::debug_bounds`.
const BRANCH_SELECTOR: &str = "status-git-branch";

/// The item's stable element id: one readout per status bar.
const BRANCH_ITEM_ID: &str = "git-branch-item";

/// The readout's text, and its tooltip's meta line: what the number counts.
const DIRTY_MEANING: &str = "changed files in the working tree";

/// The status bar's branch readout: the branch and dirty count of the workspace's
/// repository (REQ-COLLAB-03), hidden outside one.
struct BranchItem {
    /// Where git's answer comes from.
    source: Arc<dyn StatusSource>,
    /// The workspace folder to ask about — `None` while no workspace is open here.
    root: Option<PathBuf>,
    /// The last answer, the readout's source. `None` outside a repository.
    status: Option<GitStatus>,
    /// The one message of the last failure, shown inline (the S44a carry-forward).
    problem: Option<SharedString>,
    /// The refresh in flight. Replacing it cancels the answer before it lands — a
    /// dropped `Task` cancels its work (AGENTS.md), which is what a newer trigger wants.
    refreshing: Option<Task<()>>,
    /// Kept so the workspace-open and window-focus triggers keep firing; dropping them
    /// would silently stop the refreshes (AGENTS.md).
    _subscriptions: Vec<Subscription>,
}

impl BranchItem {
    /// A readout asking `source` about whatever workspace root it is later given, and
    /// re-asking whenever its window regains focus.
    fn new(source: Arc<dyn StatusSource>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _activation_observer =
            cx.observe_window_activation(window, |this: &mut Self, _window, cx| {
                this.refresh(cx);
            });
        Self {
            source,
            root: None,
            status: None,
            problem: None,
            refreshing: None,
            _subscriptions: vec![_activation_observer],
        }
    }

    /// Follow `open`'s workspace folder: every event it emits — a folder opened, the
    /// watcher's snapshot landed (REQ-COLLAB-01) — re-reads the root and refreshes.
    fn watch_open(&mut self, open: &Entity<WorkspaceOpen>, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.subscribe(open, |this, open, _event, cx| {
                let root = open.read_with(cx, |open, _cx| open.root().map(Path::to_path_buf));
                this.set_root(root, cx);
            }));
        let root = open.read_with(cx, |open, _cx| open.root().map(Path::to_path_buf));
        self.set_root(root, cx);
    }

    /// Point the readout at `root` — `None` while no workspace is open here — and
    /// refresh it.
    fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.root = root;
        self.refresh(cx);
    }

    /// Re-ask git about the current root on a background task.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            // No workspace to ask about: nothing to show, and no task to spawn.
            self.status = None;
            self.problem = None;
            cx.notify();
            return;
        };
        let source = Arc::clone(&self.source);
        self.refreshing = Some(cx.spawn(async move |this, cx| {
            let answer = cx.background_executor().spawn(source.status(root)).await;
            // The window closed while git was answering: there is no one left to show it
            // to, so the answer is dropped with the item.
            this.update(cx, |item, cx| item.apply(answer, cx)).ok();
        }));
    }

    /// Show what git answered: the readout, the hidden state outside a repository, or
    /// the failure — never a failure hidden as "no repository" (the S44a carry-forward).
    fn apply(&mut self, answer: Result<Option<GitStatus>, GitError>, cx: &mut Context<Self>) {
        match answer {
            Ok(Some(status)) => {
                self.status = Some(status);
                self.problem = None;
            }
            Ok(None) => {
                self.status = None;
                self.problem = None;
            }
            Err(error) => {
                self.status = None;
                self.problem = Some(SharedString::from(error.to_string()));
            }
        }
        cx.notify();
    }

    /// The text the strip shows: the branch and the dirty count (REQ-COLLAB-03), in the
    /// blueprint's `Console  0` shape (title, count). `None` outside a repository.
    fn readout(&self) -> Option<String> {
        self.status.as_ref().map(|status| {
            let branch = &status.branch;
            let dirty = status.dirty;
            format!("{branch}  {dirty}")
        })
    }
}

impl Render for BranchItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut item = h_flex()
            .id(BRANCH_ITEM_ID)
            .debug_selector(|| BRANCH_SELECTOR.to_owned())
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx));
        if let Some(problem) = self.problem.clone() {
            // A repository Pustman cannot read says why here, where the user is looking
            // (AGENTS.md › errors reach the user). The label truncates; the tooltip keeps
            // the full message.
            item = item
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size(IconSize::Small)
                        .color(Color::Error),
                )
                .child(
                    Label::new(problem.clone())
                        .size(LabelSize::Small)
                        .color(Color::Error)
                        .truncate(),
                )
                .tooltip(Tooltip::text(problem).builder());
        } else if let Some(readout) = self.readout() {
            let readout = SharedString::from(readout);
            item = item
                .child(
                    Icon::new(IconName::GitBranch)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(readout.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                )
                .tooltip(Tooltip::with_meta(readout, DIRTY_MEANING).builder());
        }
        item
    }
}

/// The app-wide wiring: the init guard, the windows already carrying an item, and the
/// subscription that wires the next one (the shape `crate::status_items` uses).
#[derive(Default)]
struct BranchWiring {
    started: bool,
    registered: Vec<EntityId>,
    subscriptions: Vec<Subscription>,
}

impl Global for BranchWiring {}

/// Wire the branch readout into every workspace window's status bar (REQ-COLLAB-03).
pub fn init(cx: &mut App) {
    if cx.default_global::<BranchWiring>().started {
        return;
    }
    cx.default_global::<BranchWiring>().started = true;
    let subscription = cx.observe_new::<Workspace>(|_workspace, window, cx| {
        let Some(window) = window else {
            // A workspace without a window has no status bar to fill.
            return;
        };
        let workspace = cx.entity().downgrade();
        let item = cx.new(|cx| BranchItem::new(Arc::new(GitCli), window, cx));
        // Deferred: the window is on the update stack while its workspace is being
        // created (the shape `crate::status_items` wires its readouts with).
        cx.defer(move |cx| add_branch_item(&workspace, item, cx));
    });
    cx.default_global::<BranchWiring>()
        .subscriptions
        .push(subscription);
}

/// Add the readout to one workspace's status bar, at most once per workspace whichever
/// path gets there first, and follow that window's open workspace.
fn add_branch_item(workspace: &WeakEntity<Workspace>, item: Entity<BranchItem>, cx: &mut App) {
    let Some(workspace) = workspace.upgrade() else {
        // Gone with its window before the deferred wire ran.
        return;
    };
    let workspace_id = workspace.entity_id();
    if cx
        .default_global::<BranchWiring>()
        .registered
        .contains(&workspace_id)
    {
        return;
    }
    if let Some(open) = WorkspaceOpen::for_window(workspace_id, cx) {
        item.update(cx, |item, cx| item.watch_open(&open, cx));
    }
    workspace.update(cx, |workspace, cx| {
        let status_bar = workspace.status_bar();
        status_bar.update(cx, |status_bar, cx| {
            status_bar.add_left_item(item, cx);
        });
    });
    cx.default_global::<BranchWiring>()
        .registered
        .push(workspace_id);
}

#[cfg(test)]
#[path = "status_item_tests.rs"]
mod tests;
