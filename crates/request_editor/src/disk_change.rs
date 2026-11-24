//! The "Changed on disk" bar (REQ-COLLAB-01): what an open request tab does when the
//! file behind it changes under the app's feet — a `git pull`, a branch switch, a
//! teammate's edit on a shared drive. A clean tab follows the file silently; a dirty
//! tab keeps the draft and shows "Changed on disk — Reload / Keep mine / Compare" so
//! the user decides which version wins. A rename behind the app's back moves the tab's
//! save target with it, and a delete — or a file that can no longer be read — flags the
//! tab and drops its save target instead of letting the next save resurrect or
//! overwrite a file the workspace no longer has (#308, REQ-COLLAB-02).
//!
//! The trigger is this tab's [`FsChange`]es from a real [`workspace_store::watch`] over
//! the folder its window opened ([`WorkspaceOpen`]): the same watcher the collections
//! tree feeds from (S42a), driven here so the running app's bar reacts to real watcher
//! events and the tests can prove it through the real open path
//! ([`WorkspaceOpen::open_folder`]), with nothing hand-installed.
//!
//! `Decision:` one watcher per open tab rather than a shared feed per workspace: the
//! watcher, its pump and its subscription are the tab's own fields, so closing the tab
//! or its window drops all three together and nothing needs a registry to sweep (the
//! S42a review's leaked-watcher finding cannot happen here). The cost is one notify
//! thread per open tab; a shared feed can replace this behind the same [`apply_disk_snapshot`]
//! door if a session with many tabs ever notices.
//!
//! `Decision:` a change batch re-derives the tab's state from one fresh
//! [`workspace_store::open`] — the crate's only read door — rather than being patched
//! event by event: a rename reports its two legs in either order or in two batches, and
//! "where does my request live now" is answered by the read, not by replaying events.
//!
//! `Decision:` [`FsChange::Problem`] events trigger the tab only at the tab's own path:
//! the tree owns the conflict/invalid-JSON badge (S42b), and a file in that state must
//! never silently reload the tab — it flags it and drops the save target instead, so
//! nothing is written over a merge conflict (REQ-COLLAB-02).
//!
//! `Decision:` the flag on a lost file is a banner, not a closed tab (#308 offers
//! either): the draft is the user's work and closing the tab would destroy it. The save
//! target is forgotten, so Ctrl-S explains itself ("no file in a workspace yet") rather
//! than writing the deleted file back as an orphan.
//!
//! `Decision:` Compare's rows are computed when the user opens Compare and cached in
//! [`DiskChange`] — never in `render` (the ui-render rule). The diff runs over each
//! request field's own text with `similar`, paired into side-by-side lines.
//!
//! `Decision:` the bar's three buttons are plain focusable buttons in the
//! `unresolved_warning` bar's shape — tab stops with tooltips and labels — rather than
//! new keybindings: `assets/keymaps/` is outside this step's Edits and the bar is an
//! interruption the user answers by pointing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use api_model::{HttpRequest, ItemId, Request};
use gpui::{
    AnyElement, AnyWindowHandle, App, ClickEvent, Context, Entity, Global, SharedString,
    Subscription, Task, Window,
};
use similar::{ChangeTag, TextDiff};
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen, WorkspaceOpenEvent};
use workspace_store::{FsChange, ItemBody, WatchError, WorkspaceSnapshot, WorkspaceWatcher};

use crate::RequestEditor;
use crate::save;

#[cfg(test)]
#[path = "disk_change_tests.rs"]
mod tests;

/// How often the watcher's receiver is drained (S42a's feed polls the same way): one
/// poll plus the watcher's own debounce keeps an external change well under the 1 s
/// budget REQ-COLLAB-01 gives it.
const CHANGE_POLL: Duration = Duration::from_millis(100);

/// Test hook: the changed-on-disk bar over the URL bar while the tab's file holds a
/// version the draft is keeping its changes against.
pub const CHANGED_ON_DISK_SELECTOR: &str = "changed-on-disk-bar";

/// Test hook: the bar's "Reload" button.
pub const RELOAD_SELECTOR: &str = "changed-on-disk-reload";

/// Test hook: the bar's "Keep mine" button.
pub const KEEP_MINE_SELECTOR: &str = "changed-on-disk-keep-mine";

/// Test hook: the bar's "Compare" button.
pub const COMPARE_SELECTOR: &str = "changed-on-disk-compare";

/// Test hook: the Compare panel's "Mine" column heading.
pub const COMPARE_MINE_SELECTOR: &str = "compare-mine-column";

/// Test hook: the Compare panel's "On disk" column heading.
pub const COMPARE_DISK_SELECTOR: &str = "compare-on-disk-column";

/// Test hook: the banner shown while the tab's file is gone or unreadable on disk.
pub const FILE_GONE_SELECTOR: &str = "request-file-gone";

/// The app-wide wiring: the init guard and the observation behind it
/// (`read_only_wiring`'s shape).
#[derive(Default)]
struct Wiring {
    started: bool,
    /// Kept so the new-editor observation keeps firing (AGENTS.md).
    _subscriptions: Vec<Subscription>,
}

impl Global for Wiring {}

/// Give every request tab its watcher over its window's workspace folder (the module
/// docs). Must run in `init` before any request tab exists.
pub fn init(cx: &mut App) {
    if cx.default_global::<Wiring>().started {
        return;
    }
    cx.default_global::<Wiring>().started = true;
    let subscription = cx.observe_new::<RequestEditor>(|_editor, _window, cx| {
        let entity = cx.entity();
        // Deferred like the collections panel's wiring: the window lookups below need
        // the creation update clear (`App::with_window` refuses an entity on the stack).
        cx.defer(move |cx| wire_editor(&entity, cx));
    });
    cx.default_global::<Wiring>()
        ._subscriptions
        .push(subscription);
}

/// Pair `editor` with its window's opened workspace and watch the folder it has open.
fn wire_editor(editor: &Entity<RequestEditor>, cx: &mut App) {
    let editor_id = editor.entity_id();
    let Some((opener, window_handle)) = cx.with_window(editor_id, |window, cx| {
        let opener = window
            .root::<Workspace>()
            .flatten()
            .and_then(|workspace| WorkspaceOpen::for_window(workspace.entity_id(), cx));
        (opener, window.window_handle())
    }) else {
        // Handled: the window closed before this deferred wiring ran. The tab went
        // with it and there is nothing to watch for.
        return;
    };
    let Some(opener) = opener else {
        // Handled: this window has no workspace wiring (a bare test window) — there is
        // no workspace folder to watch.
        return;
    };
    let root = opener.read_with(cx, |open, _| open.root().map(Path::to_path_buf));
    editor.update(cx, |editor, cx| {
        editor.follow_workspace(&opener, window_handle, root, cx);
    });
}

/// One row of Compare's side-by-side field diff: a field's name (its own row) or one
/// paired line of that field's diff — "Mine" (the tab's draft) left, the file right.
#[derive(Clone, Debug, PartialEq)]
pub enum CompareRow {
    /// The field whose diff follows.
    Field(String),
    /// One line of a field's diff. `None` on a side means the line exists only on the
    /// other. `field` and `line` are the row's stable key within one Compare.
    Line {
        field: &'static str,
        line: usize,
        mine: Option<String>,
        disk: Option<String>,
    },
}

impl CompareRow {
    /// The row's stable key within one Compare (the `ui-render` rule's element ids).
    fn key(&self) -> String {
        match self {
            CompareRow::Field(label) => format!("field-{label}"),
            CompareRow::Line { field, line, .. } => format!("{field}-line-{line}"),
        }
    }
}

/// The "Changed on disk" bar's state: the request as the file holds it now, and the
/// side-by-side Compare once the user opens it.
pub struct DiskChange {
    /// The request as it is on disk now — what Reload adopts and what Compare diffs the
    /// draft against.
    disk: HttpRequest,
    /// Whether Compare's side-by-side field diff is showing.
    compare_open: bool,
    /// Compare's rows, computed when Compare opens (never in `render`).
    compare_rows: Arc<Vec<CompareRow>>,
}

impl DiskChange {
    fn new(disk: HttpRequest) -> Self {
        Self {
            disk,
            compare_open: false,
            compare_rows: Arc::new(Vec::new()),
        }
    }

    /// The request as the file holds it now.
    pub fn disk(&self) -> &HttpRequest {
        &self.disk
    }

    /// Whether Compare's side-by-side diff is showing.
    pub fn compare_open(&self) -> bool {
        self.compare_open
    }

    /// Compare's rows — empty until the user opens Compare.
    pub fn rows(&self) -> &[CompareRow] {
        &self.compare_rows
    }
}

/// One tab's watcher: the [`workspace_store::watch`] pair over its workspace folder and
/// the pump that turns the watcher's [`FsChange`]es into this tab's reactions. The
/// editor holds it — dropping the tab drops the watcher and cancels the pump.
pub struct DiskWatch {
    /// The live watcher and its event receiver; dropping the pair stops the watcher.
    source: Option<(WorkspaceWatcher, Receiver<FsChange>)>,
    /// The pump: starts the watcher and re-reads the workspace on every change batch
    /// that touches this tab. Held for its lifetime only — replacing it cancels the old
    /// one, a dropped [`Task`] cancels its work (AGENTS.md), which is what a newer open
    /// wants.
    _pump: Option<Task<()>>,
    /// The file the tab's request occupied the last time the workspace was read, so a
    /// file that can no longer be read can be named in the banner (#308).
    last_path: Option<PathBuf>,
    /// Whether a re-read failure has been reported for the streak in flight (S42a fix
    /// 2's shape): one toast per streak, never one per change batch.
    read_failed: bool,
}

impl RequestEditor {
    /// Follow this tab's window's workspace: adopt the folder it has open now and
    /// re-watch on every folder it opens later (`external.rs`'s feed follows its
    /// opener's events the same way).
    fn follow_workspace(
        &mut self,
        opener: &Entity<WorkspaceOpen>,
        window_handle: AnyWindowHandle,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self._disk_open_subscription =
            Some(cx.subscribe(opener, move |editor, _opener, event, cx| {
                if let WorkspaceOpenEvent::Opened { root } = event {
                    editor.watch_disk(root.clone(), window_handle, cx);
                }
            }));
        if let Some(root) = root {
            self.watch_disk(root, window_handle, cx);
        }
    }

    /// Watch `root` for changes to this tab's file, replacing whatever it watched
    /// before (dropping the old watcher stops its events; dropping the old pump
    /// cancels it).
    fn watch_disk(
        &mut self,
        root: PathBuf,
        window_handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        self.disk_watch = None;
        let pump = cx.spawn(async move |this, cx| {
            'watcher: loop {
                // `watch` scans the folder before it arms the notify handle — blocking
                // IO, so it runs off the foreground thread (AGENTS.md › async).
                let started = cx
                    .background_spawn({
                        let root = root.clone();
                        async move { workspace_store::watch(&root) }
                    })
                    .await;
                match this.update(cx, |editor, cx| {
                    editor.store_watch(started, window_handle, cx)
                }) {
                    Ok(true) => {}
                    // Either the failure is on screen already or the tab went away
                    // with its window: nothing left to pump.
                    _ => return,
                }
                loop {
                    cx.background_executor().timer(CHANGE_POLL).await;
                    let relevant = match this.update(cx, |editor, _| editor.take_disk_changes()) {
                        Ok(Some(relevant)) => relevant,
                        // The watcher's thread is gone: re-watch and re-read, or the
                        // change in between is lost without a signal.
                        Ok(None) => continue 'watcher,
                        Err(_) => return,
                    };
                    if !relevant {
                        continue;
                    }
                    // One fresh read of the workspace per change batch (the module
                    // `Decision:`), off the foreground thread.
                    let snapshot = cx
                        .background_spawn({
                            let root = root.clone();
                            async move { workspace_store::open(&root) }
                        })
                        .await;
                    if this
                        .update_in(cx, |editor, window, cx| {
                            editor.apply_disk_snapshot(&root, snapshot, window_handle, window, cx);
                        })
                        .is_err()
                    {
                        // The tab closed while the read ran; its watcher goes with it.
                        return;
                    }
                }
            }
        });
        self.disk_watch = Some(DiskWatch {
            source: None,
            _pump: Some(pump),
            last_path: None,
            read_failed: false,
        });
    }

    /// Keep the started watcher, or tell the user why there is none. `false` stops the
    /// pump.
    fn store_watch(
        &mut self,
        started: Result<(WorkspaceWatcher, Receiver<FsChange>), WatchError>,
        window_handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(watch) = self.disk_watch.as_mut() else {
            return false;
        };
        match started {
            Ok(source) => {
                watch.source = Some(source);
                true
            }
            Err(error) => {
                save::show_toast(
                    window_handle,
                    format!("The workspace folder could not be watched — {error}"),
                    Severity::Error,
                    cx,
                );
                false
            }
        }
    }

    /// The watcher's changes that touch this tab since the last poll. `Some` with
    /// `false` means "quiet"; `None` — no watcher to drain — tells the pump to start a
    /// new one.
    fn take_disk_changes(&mut self) -> Option<bool> {
        let id = self.draft.id;
        let watch = self.disk_watch.as_mut()?;
        let (_watcher, receiver) = watch.source.as_ref()?;
        let mut relevant = false;
        loop {
            match receiver.try_recv() {
                Ok(change) => {
                    if mentions_tab(&change, id, watch.last_path.as_deref()) {
                        relevant = true;
                    }
                }
                Err(TryRecvError::Empty) => return Some(relevant),
                Err(TryRecvError::Disconnected) => break,
            }
        }
        watch.source = None;
        None
    }

    /// The workspace as the re-read found it: follow the file wherever it went (#308),
    /// reload a clean tab silently, raise the bar on a dirty one (REQ-COLLAB-01). A
    /// workspace that cannot be read is reported once per failure streak (S42a fix 2's
    /// rule) and heals on the next read.
    fn apply_disk_snapshot(
        &mut self,
        root: &Path,
        snapshot: Result<WorkspaceSnapshot, workspace_store::OpenError>,
        window_handle: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = match snapshot {
            Ok(snapshot) => {
                if let Some(watch) = self.disk_watch.as_mut() {
                    watch.read_failed = false;
                }
                snapshot
            }
            Err(error) => {
                let first = match self.disk_watch.as_mut() {
                    Some(watch) if !watch.read_failed => {
                        watch.read_failed = true;
                        true
                    }
                    _ => false,
                };
                if first {
                    save::show_toast(
                        window_handle,
                        format!(
                            "Could not re-read {} after a change on disk — {error}",
                            root.display()
                        ),
                        Severity::Error,
                        cx,
                    );
                }
                return;
            }
        };
        let id = self.draft.id;
        let Some(item) = snapshot.items.get(&id) else {
            self.flag_file_gone(self.gone_message(&snapshot), root, cx);
            return;
        };
        // A rename moves the file: the save target and this tab's story follow it
        // (#308), wherever the request went.
        if let Some(watch) = self.disk_watch.as_mut() {
            watch.last_path = item.path.clone();
        }
        save::record_placement(root, item.clone(), cx);
        let ItemBody::Request(Request::Http(disk)) = &item.body else {
            // The file is there but no longer holds an HTTP request this tab can edit.
            self.flag_file_gone(self.gone_message(&snapshot), root, cx);
            return;
        };
        let disk = disk.clone();
        if self.file_gone.take().is_some() {
            cx.notify();
        }
        if disk == self.saved {
            // The change is undone — or it was our own write echoing back through the
            // watcher (S42a's carry-forward): nothing to reload and no bar to keep.
            if self.disk_change.take().is_some() {
                cx.notify();
            }
            return;
        }
        if self.draft == self.saved {
            // A clean tab follows the file silently (REQ-COLLAB-01).
            self.reload_from_disk(disk, window, cx);
        } else if !self
            .disk_change
            .as_ref()
            .is_some_and(|change| change.disk == disk)
        {
            self.disk_change = Some(DiskChange::new(disk));
            cx.notify();
        }
    }

    /// Why the tab's request is gone from the workspace, in the user's words: the file
    /// is there but cannot be read (a merge conflict or invalid JSON — which [`workspace_store::open`]
    /// reports as a problem rather than an item, REQ-COLLAB-02), or it is gone.
    fn gone_message(&self, snapshot: &WorkspaceSnapshot) -> SharedString {
        let unreadable = self
            .disk_watch
            .as_ref()
            .and_then(|watch| watch.last_path.as_deref())
            .filter(|last_path| {
                snapshot
                    .problems
                    .iter()
                    .any(|problem| problem.path.as_path() == *last_path)
            });
        match unreadable {
            Some(path) => SharedString::from(format!(
                "{} on disk could not be read — it may contain merge-conflict markers or \
                 invalid JSON. Nothing is written over it",
                path.display()
            )),
            None => SharedString::from(format!(
                "{} is no longer on disk — this tab keeps your draft, but there is no file \
                 to save to",
                self.draft.name
            )),
        }
    }

    /// Flag the tab as having no readable file behind it (#308, REQ-COLLAB-02): the
    /// banner says what happened, and the save target is dropped so the next save
    /// cannot resurrect or overwrite a file the workspace no longer has — it reports
    /// "no file in a workspace yet" instead.
    fn flag_file_gone(&mut self, message: SharedString, root: &Path, cx: &mut Context<Self>) {
        self.disk_change = None;
        save::forget_placement(root, &self.draft.id, cx);
        if self.file_gone.as_ref() != Some(&message) {
            self.file_gone = Some(message);
            cx.notify();
        }
    }

    /// Make `request` this tab's request everywhere it shows: the draft and the saved
    /// baseline, and the URL bar, Params, Headers, Body and Auth rebuilt from it — the
    /// one construction path `RequestEditor::new` builds from, the same rebuild
    /// `curl_paste`'s import uses. A tab left holding the replaced request's state
    /// would overwrite the file's body or auth on its next edit.
    fn reload_from_disk(
        &mut self,
        request: HttpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saved = request.clone();
        self.draft = request;
        // The Auth tab keeps its parent chain: a file change replaces the request in
        // place, so the folder and collection above it are unchanged (`curl_paste`'s rule).
        let parents = self.auth_tab.read(cx).parents().to_vec();
        let fields = Self::build_request_fields(&self.draft, window, cx);
        fields
            .auth_tab
            .update(cx, |tab, cx| tab.set_parents(parents, cx));
        fields.install(self);
        // The rebuilt fields hold empty scopes until the editor's own are pushed into
        // them again — `{{variables}}` must keep resolving against one set everywhere.
        self.refresh_highlights(cx);
        self.refresh_auto_headers(cx);
        self.recheck_unresolved(cx);
        self.clear_stale_error();
        self.disk_change = None;
        self.file_gone = None;
        cx.notify();
        // The rebuilt bar is a new field to type in: the cursor goes where a user
        // editing a request starts (`curl_paste`'s rule).
        self.focus_url_bar(window, cx);
    }

    /// Reload (the bar's first way out): discard the draft and adopt the file.
    fn handle_reload(&mut self, _event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(change) = self.disk_change.take() else {
            return;
        };
        self.reload_from_disk(change.disk, window, cx);
    }

    /// Keep mine (the bar's second way out): the draft wins and the next save
    /// overwrites the file. The bar goes; the tab stays dirty until that save.
    fn handle_keep_mine(
        &mut self,
        _event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disk_change.take().is_some() {
            cx.notify();
        }
        // The button this click landed on goes with the bar; a keyboard user's focus
        // must not disappear with it (`unresolved_warning`'s rule).
        window.focus(&self.focus_handle, cx);
    }

    /// Compare (the bar's third way out): the side-by-side field diff of the draft and
    /// the file, computed here — when the user opens it — and never in `render`.
    fn handle_compare(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(change) = self.disk_change.as_mut() else {
            return;
        };
        change.compare_open = !change.compare_open;
        if change.compare_open && change.compare_rows.is_empty() {
            change.compare_rows = Arc::new(build_rows(&self.draft, &change.disk));
        }
        cx.notify();
    }

    /// The changed-on-disk bar over the URL bar: the fact, then the three ways past it.
    pub(super) fn render_disk_change(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.disk_change.as_ref()?;
        let named = save::file_for(&self.draft.id, cx)
            .and_then(|file| file.item.path)
            .map_or_else(
                || self.draft.name.clone(),
                |path| path.display().to_string(),
            );
        let tip = SharedString::from(format!(
            "{named} changed on disk — choose which version wins"
        ));
        Some(
            h_flex()
                .id(ElementId::Name(SharedString::new_static(
                    CHANGED_ON_DISK_SELECTOR,
                )))
                .debug_selector(|| CHANGED_ON_DISK_SELECTOR.to_string())
                .items_center()
                .gap(DynamicSpacing::Base03.rems(cx))
                .px(DynamicSpacing::Base03.rems(cx))
                .py(DynamicSpacing::Base02.rems(cx))
                .rounded_md()
                .border_1()
                .min_w_0()
                .border_color(Severity::Warning.border(cx))
                .bg(Severity::Warning.background(cx))
                .tooltip(Tooltip::text(tip).builder())
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Small)
                        .color(Color::Warning),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Label::new(SharedString::new_static("Changed on disk"))
                            .size(LabelSize::Small)
                            .color(Color::Warning)
                            .truncate(),
                    ),
                )
                .child(bar_button(
                    RELOAD_SELECTOR,
                    "Reload",
                    "Discard this tab's changes and load the file",
                    cx.listener(Self::handle_reload),
                ))
                .child(bar_button(
                    KEEP_MINE_SELECTOR,
                    "Keep mine",
                    "Keep this tab's draft; the next save overwrites the file",
                    cx.listener(Self::handle_keep_mine),
                ))
                .child(bar_button(
                    COMPARE_SELECTOR,
                    "Compare",
                    "Compare this tab's draft with the file side by side — click again to close",
                    cx.listener(Self::handle_compare),
                ))
                .into_any_element(),
        )
    }

    /// The file-gone banner over the URL bar: the fact, with the draft explicitly kept.
    pub(super) fn render_file_gone(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.file_gone.as_ref()?;
        Some(
            div()
                .debug_selector(|| FILE_GONE_SELECTOR.to_string())
                .child(
                    Banner::new(
                        ElementId::Name(SharedString::new_static(FILE_GONE_SELECTOR)),
                        message.clone(),
                    )
                    .severity(Severity::Error),
                )
                .into_any_element(),
        )
    }

    /// Compare's side-by-side field diff, while it is open: the columns' headings over
    /// the paired rows [`build_rows`] computed when Compare opened.
    pub(super) fn render_compare(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let change = self
            .disk_change
            .as_ref()
            .filter(|change| change.compare_open())?;
        let rows = Arc::clone(&change.compare_rows);
        let keys = Arc::clone(&change.compare_rows);
        let row_count = rows.len();
        Some(
            v_flex()
                .size_full()
                .min_h_0()
                .child(
                    h_flex()
                        .w_full()
                        .gap(DynamicSpacing::Base04.rems(cx))
                        .px(DynamicSpacing::Base04.rems(cx))
                        .py(DynamicSpacing::Base02.rems(cx))
                        .border_b_1()
                        .border_color(cx.theme().colors().border_variant)
                        .child(compare_heading(COMPARE_MINE_SELECTOR, "Mine (this tab)"))
                        .child(compare_heading(COMPARE_DISK_SELECTOR, "On disk")),
                )
                .child(VirtualList::new(
                    "changed-on-disk-diff",
                    row_count,
                    move |index| {
                        ElementId::Name(SharedString::from(
                            keys.get(index)
                                .map_or_else(|| format!("diff-row-{index}"), CompareRow::key),
                        ))
                    },
                    move |index, _window, cx| render_diff_row(&rows, index, cx),
                ))
                .into_any_element(),
        )
    }

    /// The changed-on-disk bar's state — [`Some`] while the file behind this tab holds
    /// a version its draft keeps its changes against (REQ-COLLAB-01).
    pub fn disk_change(&self) -> Option<&DiskChange> {
        self.disk_change.as_ref()
    }

    /// The banner's message while this tab's file is gone from disk or can no longer be
    /// read (#308, REQ-COLLAB-02).
    pub fn file_gone(&self) -> Option<&str> {
        self.file_gone.as_deref()
    }

    /// Compare's rows, once the user opened Compare.
    pub fn compare_rows(&self) -> Option<&[CompareRow]> {
        self.disk_change
            .as_ref()
            .filter(|change| change.compare_open())
            .map(|change| change.rows())
    }
}

/// Whether one [`FsChange`] can change what this tab shows: the tab's own item moved,
/// changed or went, a rescan that may cover its file, or a problem at the tab's own
/// path (which is what a conflicted or unparseable file reports instead of a change).
fn mentions_tab(change: &FsChange, id: ItemId, last_path: Option<&Path>) -> bool {
    match change {
        FsChange::ItemChanged { id: changed, .. }
        | FsChange::ItemAdded { id: changed, .. }
        | FsChange::ItemRemoved { id: changed, .. } => *changed == id,
        FsChange::Rescan => true,
        FsChange::Problem(problem) => last_path == Some(problem.path.as_path()),
    }
}

/// One action on the bar: a small outlined button wrapped in the debug key its test
/// clicks (`unresolved_warning`'s shape).
fn bar_button(
    selector: &'static str,
    label: &'static str,
    tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div().debug_selector(|| selector.to_string()).child(
        Button::new(
            ElementId::Name(SharedString::from(format!("{selector}-button"))),
            SharedString::new_static(label),
        )
        .size(ButtonSize::Medium)
        .style(ButtonStyle::Outlined)
        .tooltip(Tooltip::text(SharedString::new_static(tip)))
        .on_click(on_click),
    )
}

/// One column heading of Compare's diff.
fn compare_heading(selector: &'static str, label: &'static str) -> impl IntoElement {
    div()
        .debug_selector(|| selector.to_string())
        .flex_1()
        .child(
            Label::new(SharedString::new_static(label))
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
}

/// One row of Compare's diff: a field's name, or one paired line — "Mine" left, the
/// file right, each truncating with its full text in a tooltip.
fn render_diff_row(rows: &[CompareRow], index: usize, cx: &App) -> AnyElement {
    match rows.get(index) {
        Some(CompareRow::Field(label)) => h_flex()
            .w_full()
            .px(DynamicSpacing::Base04.rems(cx))
            .pt(DynamicSpacing::Base03.rems(cx))
            .child(
                Label::new(SharedString::from(format!("{label}:")))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .into_any_element(),
        Some(CompareRow::Line {
            field,
            line,
            mine,
            disk,
        }) => {
            let key = format!("{field}-line-{line}");
            h_flex()
                .w_full()
                .px(DynamicSpacing::Base04.rems(cx))
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(diff_cell(&format!("{key}-mine"), mine.as_deref()))
                .child(diff_cell(&format!("{key}-disk"), disk.as_deref()))
                .into_any_element()
        }
        None => div().into_any_element(),
    }
}

/// One side of one diff line: the text truncating in its half of the row, the full line
/// in a tooltip. A side the line does not exist on keeps its half of the grid.
fn diff_cell(key: &str, line: Option<&str>) -> AnyElement {
    let text = line.unwrap_or_default();
    div()
        .id(ElementId::Name(SharedString::from(format!("diff-{key}"))))
        .flex_1()
        .min_w_0()
        .child(
            Label::new(SharedString::from(text.to_string()))
                .size(LabelSize::Small)
                .color(if line.is_some() {
                    Color::Default
                } else {
                    Color::Hint
                })
                .truncate(),
        )
        .tooltip(Tooltip::text(SharedString::from(text.to_string())).builder())
        .into_any_element()
}

/// Compare's rows: one section per request field the two versions disagree on, each a
/// side-by-side line diff of that field's own text (`similar`).
fn build_rows(mine: &HttpRequest, disk: &HttpRequest) -> Vec<CompareRow> {
    let mut rows = Vec::new();
    for (label, mine_text, disk_text) in field_texts(mine, disk) {
        rows.push(CompareRow::Field(String::from(label)));
        push_side_by_side(&mut rows, label, &mine_text, &disk_text);
    }
    rows
}

/// The fields Compare diffs — the request's own fields, each as text — keeping only the
/// ones the two versions disagree on. Name, method and URL diff as themselves; the
/// structured fields as their pretty JSON.
fn field_texts(mine: &HttpRequest, disk: &HttpRequest) -> Vec<(&'static str, String, String)> {
    let mut fields: Vec<(&'static str, String, String)> = Vec::new();
    push_text_field(&mut fields, "Name", &mine.name, &disk.name);
    push_text_field(
        &mut fields,
        "Method",
        mine.method.as_str(),
        disk.method.as_str(),
    );
    push_text_field(&mut fields, "URL", &mine.url, &disk.url);
    push_field(&mut fields, "Query Params", &mine.params, &disk.params);
    push_field(
        &mut fields,
        "Path Variables",
        &mine.path_variables,
        &disk.path_variables,
    );
    push_field(&mut fields, "Headers", &mine.headers, &disk.headers);
    push_field(&mut fields, "Body", &mine.body, &disk.body);
    push_field(&mut fields, "Authorization", &mine.auth, &disk.auth);
    push_field(&mut fields, "Settings", &mine.settings, &disk.settings);
    push_field(&mut fields, "Examples", &mine.examples, &disk.examples);
    push_field(&mut fields, "Other Fields", &mine.extra, &disk.extra);
    fields
}

/// Keep `label`'s diff entry when the two plain-text sides differ.
fn push_text_field(
    fields: &mut Vec<(&'static str, String, String)>,
    label: &'static str,
    mine: &str,
    disk: &str,
) {
    if mine != disk {
        fields.push((label, String::from(mine), String::from(disk)));
    }
}

/// Keep `label`'s diff entry when the two sides differ; anything that serializes falls
/// back to its `Debug` shape rather than hiding the difference.
fn push_field<T: serde::Serialize + std::fmt::Debug>(
    fields: &mut Vec<(&'static str, String, String)>,
    label: &'static str,
    mine: &T,
    disk: &T,
) {
    let mine_text = serde_json::to_string_pretty(mine).unwrap_or_else(|_| format!("{mine:?}"));
    let disk_text = serde_json::to_string_pretty(disk).unwrap_or_else(|_| format!("{disk:?}"));
    if mine_text != disk_text {
        fields.push((label, mine_text, disk_text));
    }
}

/// Pair `similar`'s line diff of one field into side-by-side rows: a run of deleted
/// lines beside a run of inserted ones is one replacement shown on both sides, and a
/// line only one side has keeps its half of the row.
fn push_side_by_side(rows: &mut Vec<CompareRow>, field: &'static str, mine: &str, disk: &str) {
    let diff = TextDiff::from_lines(mine, disk);
    let mut left: Vec<String> = Vec::new();
    let mut right: Vec<String> = Vec::new();
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches(['\n', '\r']).to_string();
        match change.tag() {
            ChangeTag::Equal => {
                flush_pair(rows, field, &mut left, &mut right);
                rows.push(CompareRow::Line {
                    field,
                    line: rows.len(),
                    mine: Some(line.clone()),
                    disk: Some(line),
                });
            }
            ChangeTag::Delete => left.push(line),
            ChangeTag::Insert => right.push(line),
        }
    }
    flush_pair(rows, field, &mut left, &mut right);
}

/// The pending replacement: zipped onto both sides, the leftovers one side only.
fn flush_pair(
    rows: &mut Vec<CompareRow>,
    field: &'static str,
    left: &mut Vec<String>,
    right: &mut Vec<String>,
) {
    let pairs = left.len().min(right.len());
    for (mine, disk) in left.drain(..pairs).zip(right.drain(..pairs)) {
        rows.push(CompareRow::Line {
            field,
            line: rows.len(),
            mine: Some(mine),
            disk: Some(disk),
        });
    }
    for mine in left.drain(..) {
        rows.push(CompareRow::Line {
            field,
            line: rows.len(),
            mine: Some(mine),
            disk: None,
        });
    }
    for disk in right.drain(..) {
        rows.push(CompareRow::Line {
            field,
            line: rows.len(),
            mine: None,
            disk: Some(disk),
        });
    }
}
