//! Open and create workspaces (PRD REQ-WSP-04/05, REQ-COLLAB-06): the folder dialog
//! behind `OpenWorkspace`, the create behind `CreateWorkspace`'s form, the workspace folder one window holds
//! ([`WorkspaceOpen`], one per window — REQ-WSP-05), and the recent list the launch
//! reopen reads. [`workspace_store`] does the file work — [`workspace_store::open`] reads
//! a folder, [`workspace_store::save_item`] writes `pustman.json` — and `storage`'s
//! `recent_workspaces` records what was opened. Every bit of IO runs on a background
//! task (AGENTS.md › async) and every failure reaches the user as a toast (AGENTS.md ›
//! errors reach the user).
//!
//! `Decision:` the local store is installed through [`WorkspaceOpen::install_local_store`]
//! by whoever opens it — the app's `open_local_store`, one line in a wire step — instead
//! of being opened here: the app runs on exactly one `Arc<Mutex<LocalStore>>` (the shape
//! [`crate::Restore::attach`] is handed) and a second connection to the same database
//! would diverge from that store's in-memory fallback and race its writes. Both orders
//! work: a store that lands after the windows ran out makes
//! [`WorkspaceOpen::install_local_store`] run their launch reopen. Without a store the
//! open and create flows still work and record nothing — the wiring gap is one line, not
//! a behavior hole.
//!
//! `Decision:` a window with no workspace of its own reopens the most recently recorded
//! one (REQ-WSP-05's "the last workspace reopens at launch"). v1 has one window, so a new
//! window *is* a launch; a multi-window build would give each window its own restore.
//!
//! `Decision:` `CreateWorkspace` opens the New Workspace form
//! ([`crate::new_workspace_form`], S48u1) instead of a folder dialog: the user names the
//! workspace and picks the parent folder, and the workspace is created at
//! `<location>/<slug of name>` under the name typed. [`WorkspaceOpen::create_named`] is
//! the one create path; [`WorkspaceOpen::create_folder`] (the folder takes its own name)
//! stays for callers that already hold a folder. `OpenWorkspace` is still a folder dialog.
//!
//! `Decision:` "CreateWorkspace writes `pustman.json` + layout" is the workspace file plus
//! the PRD §6.1 folder layout — `collections/` and `environments/`. The *UI* layout is
//! per-user local state (REQ-WSP-06) and is written by [`crate::Restore`], never into the
//! workspace folder. A `pustman.json` another writer already put down is refused, never
//! overwritten: the name is claimed with one exclusive create, so creating is not an
//! accidental reset of an existing workspace.
//!
//! `Decision:` creating a workspace inside a Git repository shows REQ-COLLAB-06's note —
//! nothing to add to `.gitignore`, and no secrets in the folder (REQ-WSP-06). "Inside a
//! repository" is answered by the synchronous [`inside_git_repository`] probe rather
//! than `git::status`: that one needs a `smol::process` child, whose future is woken by
//! the async-io reactor's thread — non-determinism GPUI's test scheduler refuses, so the
//! create flow's tests flaked on it. The `git` crate keeps the jobs where git is the
//! point (the branch and dirty count of S44b's status item).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{ItemId, Workspace as WorkspaceDocument};
use gpui::{
    App, Context, Entity, EntityId, EventEmitter, Global, PathPromptOptions, Subscription, Task,
    WeakEntity,
};
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use storage::{LocalStore, RecentWorkspace};
use time::OffsetDateTime;
use ui::prelude::*;
use workspace_store::{FORMAT_VERSION, Item, ItemBody, OpenError, WorkspaceSnapshot};

use crate::Workspace;

#[cfg(test)]
#[path = "workspace_open_tests.rs"]
mod tests;

/// The one message a recent-list failure shows. `storage`'s error text can quote the row
/// it failed on and never reaches the user or a log (the rule [`crate::Restore`] follows).
const RECENT_FAILURE_MESSAGE: &str = "The workspace could not be added to the recent list";
/// One message per recent-list failure is all that reaches the user; see
/// [`RECENT_FAILURE_MESSAGE`].
const RECENT_READ_FAILURE_MESSAGE: &str = "The recent workspaces could not be read";

/// The workspace's own file (PRD §6.1) — named here because the create claims it
/// exclusively (see [`create_workspace`]).
pub(crate) const WORKSPACE_FILE: &str = "pustman.json";

/// REQ-COLLAB-06: what a workspace created inside a Git repository is told. The workspace
/// folder holds only shared files, so there is nothing to add to `.gitignore` — and
/// secrets live in the keychain, never in the folder (REQ-WSP-06).
const COLLABORATION_NOTE: &str = "This workspace is inside a Git repository. Nothing to add to \
                                 .gitignore — the workspace keeps no local files here, and \
                                 secrets are never written into it.";

/// One window's open workspace: the folder it opened and everything in it (REQ-WSP-05:
/// one workspace per window).
#[derive(Debug)]
pub struct OpenedWorkspace {
    pub root: PathBuf,
    pub snapshot: WorkspaceSnapshot,
}

/// What [`WorkspaceOpen`] tells its subscribers. S42a's watcher starts on
/// [`Self::Opened`] and re-reads [`WorkspaceOpen::snapshot`] on either variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceOpenEvent {
    /// This window opened a workspace folder.
    Opened { root: PathBuf },
    /// The open workspace's snapshot was replaced without a new open — an external change
    /// landed on disk (REQ-COLLAB-01).
    SnapshotChanged,
}

/// Why the recent list could not be recorded or read. Deliberately without detail: one
/// fixed message per failure is all the user sees (see [`RECENT_FAILURE_MESSAGE`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecentError;

/// What one open or create produced once its disk work finished: the workspace to show,
/// or the one message the user gets for it (PRD §7).
enum Outcome {
    /// The folder is this window's workspace. `recorded` false means the recent list
    /// refused its write and [`RECENT_FAILURE_MESSAGE`] goes with it.
    Landed {
        workspace: OpenedWorkspace,
        recorded: bool,
    },
    /// The folder could not become a workspace here.
    Failed { message: String, level: Severity },
    /// Nothing to do: the launch reopen found no recent workspace yet.
    Nothing,
}

/// The local store the recent list lives in, once the app installs one (see the module
/// docs). Not `Default`: absent means "not installed", which is not a failure.
pub(crate) struct OpenStore(Arc<Mutex<LocalStore>>);

impl Global for OpenStore {}

/// The app-wide wiring: the init guard, the [`WorkspaceOpen`] of every window, and the
/// subscriptions that drop a window's entry with its workspace.
#[derive(Default)]
struct Openers {
    started: bool,
    by_window: BTreeMap<EntityId, Entity<WorkspaceOpen>>,
    _subscriptions: Vec<Subscription>,
}

impl Global for Openers {}

/// The workspace folder one window has open, and the work in flight for it.
pub struct WorkspaceOpen {
    workspace: WeakEntity<Workspace>,
    open: Option<OpenedWorkspace>,
    /// The folder dialog chain in flight: a new prompt replaces — cancels — the old one,
    /// so an unanswered dialog can never open a folder the user asked for twice.
    prompt: Option<Task<()>>,
    /// The open, create or launch reopen in flight. Replacing it cancels the read before
    /// it lands, which is what a newer request for this window should do.
    opening: Option<Task<()>>,
}

impl WorkspaceOpen {
    /// The open-workspace state of `workspace`'s window (REQ-WSP-05: one per window).
    pub fn for_window(workspace: EntityId, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Openers>()
            .and_then(|openers| openers.by_window.get(&workspace))
            .cloned()
    }

    /// Install the app's local store — the one `recent_workspaces` rows go into and the
    /// launch reopen reads. Both orders work (the module docs): a window that opened
    /// before the store landed runs its launch reopen here.
    pub fn install_local_store(store: Arc<Mutex<LocalStore>>, cx: &mut App) {
        cx.set_global(OpenStore(store));
        let openers: Vec<Entity<Self>> = cx
            .try_global::<Openers>()
            .map(|openers| openers.by_window.values().cloned().collect())
            .unwrap_or_default();
        for opener in openers {
            opener.update(cx, |opener, cx| opener.reopen_last(cx));
        }
    }

    /// The store the app installed, `None` before it lands.
    pub fn local_store(cx: &App) -> Option<Arc<Mutex<LocalStore>>> {
        cx.try_global::<OpenStore>()
            .map(|store| Arc::clone(&store.0))
    }

    fn new(workspace: &Entity<Workspace>) -> Self {
        Self {
            workspace: workspace.downgrade(),
            open: None,
            prompt: None,
            opening: None,
        }
    }

    /// The folder this window has open.
    pub fn root(&self) -> Option<&Path> {
        self.open.as_ref().map(|open| open.root.as_path())
    }

    /// Everything in the open workspace, as [`workspace_store::open`] read it.
    pub fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
        self.open.as_ref().map(|open| &open.snapshot)
    }

    /// Replace the open workspace's snapshot without a new open: an external change
    /// landed on disk and its watcher applied it (REQ-COLLAB-01).
    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        let root = snapshot.root.clone();
        self.open = Some(OpenedWorkspace { root, snapshot });
        cx.emit(WorkspaceOpenEvent::SnapshotChanged);
        // Views that read this window's workspace while drawing (the welcome pane) redraw.
        cx.notify();
    }

    /// Ask for the folder to open as this window's workspace (the `OpenWorkspace` action).
    pub fn prompt_open(&mut self, cx: &mut Context<Self>) {
        self.prompt_folder(cx);
    }

    /// The platform folder dialog `OpenWorkspace` asks its question with.
    fn prompt_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::new_static("Open workspace")),
        });
        self.prompt = Some(cx.spawn(async move |this, cx| {
            let chosen = match receiver.await {
                Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
                Ok(Err(error)) => {
                    // The dialog itself failed to open — the user asked for one and gets
                    // the reason it is not on screen. A window that closed meanwhile
                    // cannot show it.
                    this.update(cx, |opener, cx| {
                        opener.report(
                            format!("The folder dialog could not be opened — {error}"),
                            Severity::Error,
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
                // The dialog went away with its window.
                Err(_canceled) => return,
            };
            let Some(root) = chosen else {
                // The user cancelled the dialog: nothing was asked of the workspace.
                return;
            };
            // A window that closed between the choice and this callback has nothing to
            // open into.
            this.update(cx, |opener, cx| opener.open_folder(root, cx))
                .ok();
        }));
    }

    /// Open the workspace folder at `root` in this window: the dialog's answer, the
    /// recent picker's Enter (S41b) and the launch reopen all land here.
    pub fn open_folder(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let store = Self::local_store(cx);
        self.opening = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { open_off_foreground(root, store) })
                .await;
            // The window closed while its folder was being read; there is no one left
            // to show it to.
            this.update(cx, |opener, cx| opener.land(outcome, cx)).ok();
        }));
    }

    /// Create a workspace in the folder at `root`, named after the folder, and open it
    /// here (REQ-WSP-05). Failures reach the user as a toast.
    pub fn create_folder(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let name = workspace_name(&root);
        self.create(root, name, false, |_finished, _cx| false, cx);
    }

    /// Create a workspace called `name` in the folder at `root` and open it here
    /// (REQ-WSP-05, REQ-COLLAB-06's note included). `on_finish` hears how it went once the
    /// disk work is done: `Ok` after the workspace is open, `Err` with the reason it
    /// could not be created. It returns whether it showed a failure to the user itself
    /// (the New Workspace form shows it inline); a failure nobody showed becomes a toast,
    /// so it always reaches the user.
    ///
    /// `root` is a NEW workspace folder: one that already exists with anything in it is
    /// refused ([`CreateWorkspaceError::NotEmpty`]), checked again here on the background
    /// task, so a folder that filled up after the form validated it is still not adopted.
    pub fn create_named(
        &mut self,
        root: PathBuf,
        name: String,
        on_finish: impl FnOnce(Result<(), SharedString>, &mut App) -> bool + 'static,
        cx: &mut Context<Self>,
    ) {
        self.create(root, name, true, on_finish, cx);
    }

    /// The one create path behind [`Self::create_folder`] and [`Self::create_named`].
    /// `require_empty` refuses a folder that exists with anything in it.
    fn create(
        &mut self,
        root: PathBuf,
        name: String,
        require_empty: bool,
        on_finish: impl FnOnce(Result<(), SharedString>, &mut App) -> bool + 'static,
        cx: &mut Context<Self>,
    ) {
        let store = Self::local_store(cx);
        self.opening = Some(cx.spawn(async move |this, cx| {
            let (outcome, in_repository) = cx
                .background_executor()
                .spawn(async move {
                    let in_repository = inside_git_repository(&root);
                    (
                        create_off_foreground(root, name, require_empty, store),
                        in_repository,
                    )
                })
                .await;
            if let Outcome::Failed { message, level } = outcome {
                let reason = SharedString::from(message.clone());
                let shown = cx.update(|cx| on_finish(Err(reason), cx));
                if !shown {
                    // The window closed before the failure could be shown.
                    this.update(cx, |opener, cx| {
                        opener.land(Outcome::Failed { message, level }, cx);
                    })
                    .ok();
                }
                return;
            }
            // The window closed before its new workspace could be shown.
            this.update(cx, |opener, cx| opener.land(outcome, cx)).ok();
            // REQ-COLLAB-06's note, for a workspace created inside a repository. The
            // probe ran with the create's own disk work (see [`inside_git_repository`]);
            // a window that closed meanwhile cannot show the note.
            if in_repository {
                this.update(cx, |opener, cx| {
                    opener.report(COLLABORATION_NOTE, Severity::Info, cx);
                })
                .ok();
            }
            // Success needs no message from the caller: the open workspace is the news.
            cx.update(|cx| on_finish(Ok(()), cx));
        }));
    }

    /// The launch reopen (REQ-WSP-05): bring the most recently recorded workspace back
    /// into a window that has none yet. A window whose store is not installed waits —
    /// [`Self::install_local_store`] runs this again when it lands.
    pub fn reopen_last(&mut self, cx: &mut Context<Self>) {
        // The launch reopen defers to whatever this window is already doing — a workspace
        // it is opening, or a folder dialog waiting for its answer. A store landing
        // mid-flow must never drop the user's own open for the recent list's (the
        // review's MEDIUM).
        if self.open.is_some() || self.opening.is_some() || self.prompt.is_some() {
            return;
        }
        let Some(store) = Self::local_store(cx) else {
            return;
        };
        self.opening = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { reopen_off_foreground(store) })
                .await;
            // The window closed before its workspace reopened.
            this.update(cx, |opener, cx| opener.land(outcome, cx)).ok();
        }));
    }

    /// Show what an open, create or reopen produced: the workspace, or its one message.
    fn land(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        match outcome {
            Outcome::Landed {
                workspace,
                recorded,
            } => self.apply(workspace, recorded, cx),
            Outcome::Failed { message, level } => self.report(message, level, cx),
            Outcome::Nothing => {}
        }
    }

    /// Make `workspace` this window's, and say so — S42a's watcher and tree read the
    /// snapshot through this window's [`WorkspaceOpen`] from here on.
    fn apply(&mut self, workspace: OpenedWorkspace, recorded: bool, cx: &mut Context<Self>) {
        let root = workspace.root.clone();
        self.open = Some(workspace);
        cx.emit(WorkspaceOpenEvent::Opened { root });
        cx.notify();
        if !recorded {
            self.report(RECENT_FAILURE_MESSAGE, Severity::Warning, cx);
        }
    }

    /// Show `message` on this window's workspace. Deferred like `request_editor::save`'s
    /// toast reach-through: this runs inside an entity update of its own, and the
    /// workspace may be on the update stack above it.
    fn report(&self, message: impl Into<SharedString>, level: Severity, cx: &mut Context<Self>) {
        let message = message.into();
        let workspace = self.workspace.clone();
        cx.defer(move |cx| {
            let shown = workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message.clone(), level, None, cx);
            });
            if shown.is_err() {
                // The window is gone — there is no user left to reach, and the message is
                // all that survives of the failure.
                log::error!("{message}");
            }
        });
    }
}

impl EventEmitter<WorkspaceOpenEvent> for WorkspaceOpen {}

/// Wire the workspace-opening actions and the per-window state into the app's init order.
/// Registered app-level (`init` → `App::on_action`), the shape `request_editor::save`
/// uses: this module's Edits keep `workspace.rs` to `mod`/init lines, so there is no
/// element of ours for the keymap to dispatch to before these run.
pub fn init(cx: &mut App) {
    if cx.default_global::<Openers>().started {
        return;
    }
    cx.default_global::<Openers>().started = true;
    cx.on_action(open_workspace_action);
    cx.on_action(create_workspace_action);
    let subscription = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let workspace = cx.entity();
        attach(&workspace, cx);
    });
    cx.default_global::<Openers>()
        ._subscriptions
        .push(subscription);
}

/// Give `workspace`'s window its [`WorkspaceOpen`], and run the launch reopen it owes
/// (the module docs). Idempotent: one opener per window, whichever caller gets here first.
fn attach(workspace: &Entity<Workspace>, cx: &mut App) {
    let workspace_id = workspace.entity_id();
    if cx
        .default_global::<Openers>()
        .by_window
        .contains_key(&workspace_id)
    {
        return;
    }
    let opener = cx.new(|_cx| WorkspaceOpen::new(workspace));
    let release = cx.observe_release(workspace, move |_workspace, cx| {
        cx.default_global::<Openers>()
            .by_window
            .remove(&workspace_id);
    });
    cx.default_global::<Openers>()
        .by_window
        .insert(workspace_id, opener.clone());
    cx.default_global::<Openers>()._subscriptions.push(release);
    opener.update(cx, |opener, cx| opener.reopen_last(cx));
}

/// The `OpenWorkspace` answer in the active window.
fn open_workspace_action(_: &OpenWorkspace, cx: &mut App) {
    with_active_opener(cx, |opener, cx| opener.prompt_open(cx));
}

/// The window an app-level action (New workspace…, Import…) acts in: the active window, or
/// — when the platform reports none, as under a compositor without focus tracking or a bare
/// X server — the first open window, so the click is never silently dropped.
pub fn action_window(cx: &App) -> Option<gpui::AnyWindowHandle> {
    cx.active_window()
        .or_else(|| cx.windows().into_iter().next())
}

/// The `CreateWorkspace` answer in the active window: the New Workspace form.
fn create_workspace_action(_: &CreateWorkspace, cx: &mut App) {
    let Some(window_handle) = action_window(cx) else {
        return;
    };
    // Deferred like [`with_active_opener`]: the dispatch runs with the window on the
    // update stack.
    cx.defer(move |cx| {
        let opened = window_handle.update(cx, |root, window, cx| {
            if let Ok(workspace) = root.downcast::<Workspace>() {
                crate::new_workspace_form::open(&workspace, window, cx);
            }
        });
        if opened.is_err() {
            // Handled: the window closed before the action ran; there is no one to ask.
        }
    });
}

/// Ask the active window's [`WorkspaceOpen`] to run `prompt`. Deferred like
/// `command_palette`'s toggle: the action dispatch runs while the window is on the update
/// stack, and a window cannot be updated from there.
pub(crate) fn with_active_opener(
    cx: &mut App,
    prompt: impl FnOnce(&mut WorkspaceOpen, &mut Context<WorkspaceOpen>) + 'static,
) {
    let Some(window_handle) = action_window(cx) else {
        return;
    };
    cx.defer(move |cx| {
        let asked = window_handle.update(cx, |root, _window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return false;
            };
            let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
                return false;
            };
            opener.update(cx, |opener, cx| {
                prompt(opener, cx);
            });
            true
        });
        if !matches!(asked, Ok(true)) {
            // Handled: the window closed before the action ran. Nothing was asked of a
            // workspace, so there is nothing to tell anyone.
        }
    });
}

/// Create the workspace `root` holds from now on: its `pustman.json` and the PRD §6.1
/// folder layout. A folder that already holds a workspace file is refused, whatever that
/// file's state — creating never overwrites one (the module docs).
pub fn create_workspace(root: &Path, name: &str) -> Result<OpenedWorkspace, CreateWorkspaceError> {
    std::fs::create_dir_all(root).map_err(|error| CreateWorkspaceError::Unwritable {
        root: root.to_path_buf(),
        reason: error.to_string(),
    })?;
    let document = WorkspaceDocument {
        id: ItemId::new(),
        name: name.to_owned(),
        format_version: FORMAT_VERSION.to_owned(),
        extra: BTreeMap::new(),
    };
    let claimed_bytes =
        serde_json::to_vec(&document).map_err(|error| CreateWorkspaceError::Unwritable {
            root: root.to_path_buf(),
            reason: error.to_string(),
        })?;
    let item = Item {
        body: ItemBody::Workspace(document),
        dir: PathBuf::new(),
        path: None,
    };
    // `pustman.json` is claimed with one exclusive create, never a check-then-write: a
    // file another writer put there owns the name, and this create refuses instead of
    // overwriting it (the review's LOW). The claim carries the new workspace's own
    // document, so the name is never held by something unreadable.
    let claimed = root.join(WORKSPACE_FILE);
    let mut claim = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&claimed)
    {
        Ok(claim) => claim,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CreateWorkspaceError::AlreadyAWorkspace(root.to_path_buf()));
        }
        Err(error) => {
            return Err(CreateWorkspaceError::Unwritable {
                root: root.to_path_buf(),
                reason: error.to_string(),
            });
        }
    };
    let mut failure = std::io::Write::write_all(&mut claim, &claimed_bytes).map_err(|error| {
        CreateWorkspaceError::Unwritable {
            root: root.to_path_buf(),
            reason: error.to_string(),
        }
    });
    drop(claim);
    if failure.is_ok() {
        failure = write_layout_and_document(root, &item);
    }
    if let Err(error) = failure {
        // The failed create drops the name it claimed, so the folder is left as it was
        // found. `.ok()`: the claim may already be gone, which is the goal here.
        std::fs::remove_file(&claimed).ok();
        return Err(error);
    }
    let snapshot =
        workspace_store::open(root).map_err(|error| CreateWorkspaceError::Unreadable {
            root: root.to_path_buf(),
            reason: error.to_string(),
        })?;
    Ok(OpenedWorkspace {
        root: root.to_path_buf(),
        snapshot,
    })
}

/// [`create_workspace`] for a folder the user named as new: it may be missing or empty,
/// but a folder with anything in it is refused rather than adopted. A folder that already
/// holds `pustman.json` is still [`CreateWorkspaceError::AlreadyAWorkspace`].
pub fn create_workspace_in_new_folder(
    root: &Path,
    name: &str,
) -> Result<OpenedWorkspace, CreateWorkspaceError> {
    if !root.join(WORKSPACE_FILE).exists() && folder_has_entries(root) {
        return Err(CreateWorkspaceError::NotEmpty(root.to_path_buf()));
    }
    create_workspace(root, name)
}

/// Whether `path` exists as anything other than an empty folder — a file there, or a
/// folder with at least one entry. A path that cannot be inspected at all counts as not
/// occupied: the create then fails on it and reports the real reason.
pub(crate) fn folder_has_entries(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_dir() => true,
        Ok(_) => std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some()),
        Err(_) => false,
    }
}

/// The PRD §6.1 folder layout and the deterministic `pustman.json` write — everything
/// between claiming the workspace name and the folder being a workspace
/// (see [`create_workspace`]).
fn write_layout_and_document(root: &Path, item: &Item) -> Result<(), CreateWorkspaceError> {
    for directory in ["collections", "environments"] {
        std::fs::create_dir_all(root.join(directory)).map_err(|error| {
            CreateWorkspaceError::Unwritable {
                root: root.to_path_buf(),
                reason: error.to_string(),
            }
        })?;
    }
    workspace_store::save_item(root, item).map_err(|error| CreateWorkspaceError::Unwritable {
        root: root.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(())
}

/// Read the workspace folder at `root` into its snapshot (PRD §6.1).
pub fn open_workspace(root: &Path) -> Result<OpenedWorkspace, OpenWorkspaceError> {
    let snapshot = workspace_store::open(root).map_err(|error| match error {
        OpenError::NotAWorkspace(path) => OpenWorkspaceError::NotAWorkspace(path),
        OpenError::WorkspaceFile { path, kind } => OpenWorkspaceError::UnreadableWorkspaceFile {
            path: root.join(path),
            kind,
        },
        OpenError::Io(error) => OpenWorkspaceError::Unreadable {
            root: root.to_path_buf(),
            reason: error.to_string(),
        },
    })?;
    Ok(OpenedWorkspace {
        root: root.to_path_buf(),
        snapshot,
    })
}

/// Why a folder could not become this window's workspace. The `Display` text is the toast.
#[derive(Debug, thiserror::Error)]
pub enum OpenWorkspaceError {
    /// The folder holds no `pustman.json`, so it is not a workspace (REQ-WSP-05).
    #[error("{0} is not a Pustman workspace: pustman.json is missing")]
    NotAWorkspace(PathBuf),
    /// `pustman.json` cannot be read as a workspace file (REQ-COLLAB-02's problems are
    /// per item file and land in the snapshot instead).
    #[error("{path} could not be read: {kind}")]
    UnreadableWorkspaceFile {
        path: PathBuf,
        kind: workspace_store::ItemProblemKind,
    },
    /// The folder could not be read at all.
    #[error("{root} could not be read: {reason}")]
    Unreadable { root: PathBuf, reason: String },
}

/// Why a folder could not become a new workspace. The `Display` text is the toast.
#[derive(Debug, thiserror::Error)]
pub enum CreateWorkspaceError {
    /// The folder already holds a `pustman.json`; creating would overwrite a workspace.
    #[error("{0} already holds a workspace — open it instead")]
    AlreadyAWorkspace(PathBuf),
    /// The new workspace's folder already exists with something in it.
    #[error("{0} already exists and isn't empty. Choose another name or location.")]
    NotEmpty(PathBuf),
    /// The workspace file or its layout could not be written.
    #[error("{root} could not be created: {reason}")]
    Unwritable { root: PathBuf, reason: String },
    /// The workspace was written but could not be read back.
    #[error("{root} could not be read back: {reason}")]
    Unreadable { root: PathBuf, reason: String },
}

/// The disk and database work of one open, off the foreground thread (AGENTS.md › async).
fn open_off_foreground(root: PathBuf, store: Option<Arc<Mutex<LocalStore>>>) -> Outcome {
    match open_workspace(&root) {
        Ok(workspace) => Outcome::Landed {
            recorded: record_recent(&root, store.as_deref()),
            workspace,
        },
        Err(error) => Outcome::Failed {
            message: error.to_string(),
            level: Severity::Error,
        },
    }
}

/// The disk and database work of one create, off the foreground thread (AGENTS.md ›
/// async). The name comes with it: the form's Name, or the folder's own name for
/// [`WorkspaceOpen::create_folder`] (the module docs).
fn create_off_foreground(
    root: PathBuf,
    name: String,
    require_empty: bool,
    store: Option<Arc<Mutex<LocalStore>>>,
) -> Outcome {
    let created = if require_empty {
        create_workspace_in_new_folder(&root, &name)
    } else {
        create_workspace(&root, &name)
    };
    match created {
        Ok(workspace) => Outcome::Landed {
            recorded: record_recent(&root, store.as_deref()),
            workspace,
        },
        Err(error) => Outcome::Failed {
            message: error.to_string(),
            level: Severity::Error,
        },
    }
}

/// The launch reopen's disk and database work, off the foreground thread (AGENTS.md ›
/// async). A missing folder is a warning, not an error: the window still opens, and the
/// user can pick another workspace.
fn reopen_off_foreground(store: Arc<Mutex<LocalStore>>) -> Outcome {
    let root = match read_most_recent(&store) {
        Ok(Some(root)) => root,
        Ok(None) => return Outcome::Nothing,
        Err(RecentError) => {
            return Outcome::Failed {
                message: RECENT_READ_FAILURE_MESSAGE.to_owned(),
                level: Severity::Warning,
            };
        }
    };
    match open_workspace(&root) {
        Ok(workspace) => Outcome::Landed {
            recorded: record_recent(&root, Some(store.as_ref())),
            workspace,
        },
        Err(error) => Outcome::Failed {
            message: format!("The last workspace could not be reopened — {error}"),
            level: Severity::Warning,
        },
    }
}

/// Record `root` as opened now in the recent list (REQ-WSP-05). `false` means the write
/// failed and the caller shows [`RECENT_FAILURE_MESSAGE`]; no installed store is not a
/// failure — recording is simply not wired yet (the module docs).
fn record_recent(root: &Path, store: Option<&Mutex<LocalStore>>) -> bool {
    let Some(store) = store else {
        return true;
    };
    let recent = RecentWorkspace {
        path: root.to_string_lossy().into_owned(),
        opened_at: OffsetDateTime::now_utc(),
    };
    match store.lock() {
        Ok(store) => store.recent_workspaces().insert(&recent).is_ok(),
        // A poisoned lock means another writer panicked mid-row; the write is lost and
        // the caller says so.
        Err(_poisoned) => false,
    }
}

/// The workspace folder to reopen at launch: the most recently recorded one (REQ-WSP-05).
fn read_most_recent(store: &Mutex<LocalStore>) -> Result<Option<PathBuf>, RecentError> {
    let store = store.lock().map_err(|_poisoned| RecentError)?;
    let recents = store.recent_workspaces().list().map_err(|_| RecentError)?;
    Ok(recents
        .into_iter()
        .next()
        .map(|recent| PathBuf::from(recent.path)))
}

/// The name a created workspace takes: its folder's name (the module docs).
fn workspace_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("Workspace"))
}

/// Whether `root` sits inside a Git repository: a `.git` entry — the directory of a
/// repository or the file a worktree or submodule keeps — in the folder or one of its
/// ancestors. See the module docs for why this is not `git::status`.
fn inside_git_repository(root: &Path) -> bool {
    root.ancestors()
        .any(|directory| directory.join(".git").exists())
}
