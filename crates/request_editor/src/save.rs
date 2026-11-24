//! Saving the active request to its file (`Ctrl-S` / `Cmd-S`, REQ-WSP-01).
//!
//! `workspace::Save` writes the tab's [`api_model::HttpRequest`] draft through
//! [`workspace_store::save_item`] — the workspace format's deterministic, atomic write
//! (REQ-WSP-02) — on a background task, and clears the tab's dirty dot (REQ-UI-02) with
//! exactly the draft that landed: an edit typed while the write ran keeps the tab dirty.
//! A write that cannot land reports a toast naming the file and the reason, and so does a
//! request with no file in a workspace yet (AGENTS.md › errors reach the user).
//!
//! **One write at a time per tab.** Two detached writes on the same file race: whichever
//! lands last wins, so an older draft can overwrite a newer one and the dirty dot can
//! then lie. A press while a write is in flight therefore marks "save again" instead of
//! starting a second write, and when the write lands the tab saves its draft once more
//! if it differs from what was written — re-reading its place on disk first, because the
//! write that just landed may have renamed or moved the file (REQ-WSP-03). A write is
//! never cancelled: it runs to completion, so a rename cannot be cut in half.
//!
//! **Only the request that has the keyboard saves.** `workspace::Save` is bound in the
//! `Workspace` context and reaches the handler even while a dock panel, a modal or a
//! picker holds focus, so the handler checks the active editor's focus before it writes
//! (see [`init`]).
//!
//! Where each request's file lives comes from the [`RequestFiles`] registry: the workspace
//! root and the request's [`Item`] placement in it, which is what
//! [`workspace_store::save_item`] needs to keep a write on "its file" and what a failure
//! toast names.
//!
//! `Decision:` the `workspace::Save` handler is registered app-level ([`init`] →
//! `App::on_action`), the shape [`crate::new_request`] uses for `workspace::NewRequest`,
//! rather than an `.on_action` on the editor's root element: this step's Edits keep
//! `request_editor.rs` to `mod`/init lines, so no line may join its `render`. Like
//! `new_request`'s `focus_url_bar`, the handler routes to the request tab that is active
//! in the active window, and the focus check above stands in for the dispatch the
//! element-level handler would have had. It cannot "propagate" the unhandled action
//! instead: this is the last handler in the bubble chain and it runs in a deferred
//! callback, after the dispatch it belongs to has finished.
//!
//! `Decision:` the in-flight marker lives in [`PendingWrites`], keyed by the editor's
//! [`gpui::EntityId`] — the [`crate::send`] `SendOwner` shape — not on `RequestEditor`:
//! the same Edits keep the editor from growing a field for it. The table is empty between
//! writes, so nothing accumulates as tabs open and close.
//!
//! `Decision:` a failed write ends the save cycle. The user is told (or it is logged) and
//! the tab stays dirty for them to try again; an automatic retry would only repeat the
//! failure and its toast. A "save again" marked before the failure is dropped with it.
//!
//! `Decision:` the write task is `.detach()`ed, not stored on the editor: a write the user
//! asked for must land even if the tab closes while it runs, and dropping a `Task` would
//! cancel it. The completion takes the "editor gone" path explicitly, and the write is
//! the workspace store's atomic one either way.
//!
//! `Decision:` a successful save records the placement the store actually wrote back into
//! [`RequestFiles`] (`record_placement`), whether or not the editor still exists: a rename
//! moves the file (REQ-WSP-03) and the next save or failure toast must speak about where
//! the request lives now.
//!
//! `Decision:` [`show_toast`] is this crate's copy of the deferred
//! `Workspace::show_toast` reach-through `response_viewer`'s body save runs: `Workspace`
//! exposes only the method itself, and promoting the shared reach-through is outside this
//! step's Edits. One idea, two small helpers, until a third wants it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use api_model::{ItemId, Request};
use gpui::{AnyWindowHandle, App, Context, EntityId, Focusable, Global, Window};
use pustman_actions::workspace::Save;
use ui::prelude::*;
use workspace::Workspace;
use workspace_store::{Item, ItemBody};

use crate::RequestEditor;

/// Where one request tab's request lives on disk: the workspace root it is saved under
/// and the [`Item`] placement the workspace store wrote it to — the file it occupies
/// today and the directory its name's slug is placed in (REQ-WSP-01/03).
#[derive(Clone)]
pub struct RequestFile {
    pub root: PathBuf,
    pub item: Item,
}

/// Every request file the app knows. The workspace's open wiring installs what its
/// snapshot read ([`install`]); a save looks its own request up ([`file_for`]).
///
/// A plain list, not a map: it holds the open tabs' requests and [`file_for`] matches one
/// by the id inside the item, so a map would need the very id extraction it would then be
/// keyed by.
pub struct RequestFiles(Vec<RequestFile>);

impl Global for RequestFiles {}

/// Install `files` as the request files the app saves into — the workspace wiring's one
/// door in (and a test's).
pub fn install(files: impl IntoIterator<Item = RequestFile>, cx: &mut App) {
    cx.set_global(RequestFiles(files.into_iter().collect()));
}

/// Where the request `id` lives on disk, when the app knows a file for it.
pub fn file_for(id: &ItemId, cx: &App) -> Option<RequestFile> {
    cx.try_global::<RequestFiles>().and_then(|files| {
        files
            .0
            .iter()
            .find(|file| file.item.id() == Some(*id))
            .cloned()
    })
}

/// Keep [`RequestFiles`]' copy of `item` in step with the file a save just wrote, so a
/// later save or failure toast speaks about where the request lives now.
fn record_placement(root: &Path, item: Item, cx: &mut App) {
    let Some(files) = cx.try_global::<RequestFiles>() else {
        return;
    };
    let mut files = files.0.clone();
    for file in &mut files {
        if file.root == root && file.item.id() == item.id() {
            file.item = item.clone();
        }
    }
    cx.set_global(RequestFiles(files));
}

/// The write in flight for each editor that has one — the serialisation that keeps
/// [`workspace_store::save_item`] to one write at a time per tab (see the module docs).
/// An entry exists exactly while its write runs.
struct PendingWrites(HashMap<EntityId, PendingWrite>);

impl Global for PendingWrites {}

/// One editor's write cycle: a write running now, and whether a further `workspace::Save`
/// press asked for another one while it ran ("save again").
#[derive(Clone)]
struct PendingWrite {
    save_again: bool,
}

/// Run `change` against the pending-write table and store the result back — the one door
/// in and out, so a table that is not installed yet is an empty one, never a panic.
fn with_pending_writes<R>(
    cx: &mut App,
    change: impl FnOnce(&mut HashMap<EntityId, PendingWrite>) -> R,
) -> R {
    let mut table = cx
        .try_global::<PendingWrites>()
        .map_or_else(HashMap::new, |writes| writes.0.clone());
    let result = change(&mut table);
    cx.set_global(PendingWrites(table));
    result
}

/// `owner`'s write has started: a press from here on means "save again".
fn write_started(owner: EntityId, cx: &mut App) {
    with_pending_writes(cx, |writes| {
        writes.insert(owner, PendingWrite { save_again: false });
    });
}

/// A `workspace::Save` press while `owner`'s write runs: marks "save again" and reports
/// that a write is already in flight — so the press starts no second one.
fn mark_save_again(owner: EntityId, cx: &mut App) -> bool {
    with_pending_writes(cx, |writes| match writes.get_mut(&owner) {
        Some(pending) => {
            pending.save_again = true;
            true
        }
        None => false,
    })
}

/// `owner`'s write has finished; reports whether another save was asked for while it ran.
fn write_finished(owner: EntityId, cx: &mut App) -> bool {
    with_pending_writes(cx, |writes| {
        writes
            .remove(&owner)
            .is_some_and(|pending| pending.save_again)
    })
}

/// Register the `workspace::Save` handler (see the module docs).
pub fn init(cx: &mut App) {
    cx.on_action(save_active_request);
}

/// `workspace::Save` (Ctrl-S / Cmd-S) for the request tab active in the active window —
/// and only while that editor has the keyboard. Any other kind of tab is not a request to
/// save and is left alone.
fn save_active_request(_: &Save, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                let editor = workspace.read_with(cx, |workspace, cx| {
                    workspace
                        .center_pane()
                        .read(cx)
                        .active_item()
                        .map(|item| item.to_any_view())
                });
                let Some(editor) = editor.and_then(|view| view.downcast::<RequestEditor>().ok())
                else {
                    return;
                };
                // The active tab is not enough: the keystroke reaches this handler even
                // while a dock panel, a modal or a picker has the keyboard, and only the
                // request the user is working in may write its file behind them.
                let has_focus = editor.read_with(cx, |editor, cx| {
                    editor.focus_handle(cx).contains_focused(window, cx)
                });
                if !has_focus {
                    return;
                }
                editor.update(cx, |editor, cx| editor.save(window, cx));
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is no
            // request left to save.
        }
    });
}

impl RequestEditor {
    /// Write this tab's draft to the request's file on a background task (REQ-WSP-01),
    /// one write at a time: a press while the previous write runs is the "save again"
    /// that write's completion carries out (see the module docs). The tab is marked clean
    /// with exactly the draft that landed, so an edit typed while the write ran keeps the
    /// tab dirty; a write that cannot land is reported to the user, never only logged.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if mark_save_again(cx.entity_id(), cx) {
            return;
        }
        self.write_draft(window.window_handle(), cx);
    }

    /// Start the one write of the draft as it is now: re-read the request's placement
    /// ([`record_placement`] keeps it in step with the file), then write on a background
    /// task. `window_handle` names the window whose workspace reports failures — the
    /// re-save path has no `&mut Window` to take it from.
    fn write_draft(&mut self, window_handle: AnyWindowHandle, cx: &mut Context<Self>) {
        let owner = cx.entity_id();
        let Some(file) = file_for(&self.draft.id, cx) else {
            show_toast(
                window_handle,
                format!(
                    "Could not save {} — it has no file in a workspace yet",
                    self.draft.name
                ),
                Severity::Warning,
                cx,
            );
            return;
        };
        let target = target_name(&file.root, &file.item);
        let root = file.root;
        let mut item = file.item;
        item.body = ItemBody::Request(Request::Http(self.draft.clone()));
        let written_draft = self.draft.clone();
        write_started(owner, cx);
        cx.spawn(async move |this, cx| {
            let written = cx
                .background_spawn({
                    let root = root.clone();
                    let item = item.clone();
                    async move { workspace_store::save_item(&root, &item) }
                })
                .await;
            match written {
                Ok(path) => {
                    item.path = Some(path);
                    cx.update(|cx| record_placement(&root, item, cx));
                    let save_again = cx.update(|cx| write_finished(owner, cx));
                    if let Err(_released) = this.update(cx, |editor, cx| {
                        // Exactly the draft that landed: an edit typed while the write
                        // ran leaves the tab dirty against this one.
                        editor.saved = written_draft;
                        cx.notify();
                        if save_again && editor.draft != editor.saved {
                            // "Save again": the press asked for another save while this
                            // write ran, and the draft moved on — write it as it is now,
                            // re-reading its place (the write may have renamed the file).
                            editor.write_draft(window_handle, cx);
                        }
                    }) {
                        // The tab closed while the write ran; its file holds the request
                        // all the same and the registry still records where.
                    }
                }
                Err(error) => cx.update(|cx| {
                    write_finished(owner, cx);
                    show_toast(
                        window_handle,
                        format!("Could not save {target} — {error}"),
                        Severity::Error,
                        cx,
                    );
                }),
            }
        })
        .detach();
    }
}

/// What a save's messages name: the file the write targets, or the request's name for one
/// that has never been written and so has no file name yet.
fn target_name(root: &Path, item: &Item) -> String {
    item.path
        .as_deref()
        .map_or_else(|| item.name(), |path| root.join(path).display().to_string())
}

/// Show `message` as a toast over the workspace shell hosting `window_handle`
/// (`Workspace::show_toast` is the one door in — the response body save reaches it the
/// same way, see the module docs). The update is deferred: a window cannot be updated
/// while it is on the update stack. A message that cannot be shown — the window closed,
/// or its root is not a [`Workspace`] (a bare test window) — is logged instead, so a
/// failure is never dropped without a trace (AGENTS.md › errors reach the user). The
/// message names the file and the reason only: never a request body, headers or secrets.
fn show_toast(
    window_handle: AnyWindowHandle,
    message: impl Into<SharedString>,
    level: Severity,
    cx: &mut App,
) {
    let message = message.into();
    cx.defer(move |cx| {
        let shown = window_handle.update(cx, |root, _window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return false;
            };
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message.clone(), level, None, cx);
            });
            true
        });
        if !matches!(shown, Ok(true)) {
            log::error!("{message}");
        }
    });
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
