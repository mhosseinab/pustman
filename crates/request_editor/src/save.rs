//! Saving the active request to its file (`Ctrl-S` / `Cmd-S`, REQ-WSP-01).
//!
//! `workspace::Save` writes the tab's [`api_model::HttpRequest`] draft through
//! [`workspace_store::save_item`] — the workspace format's deterministic, atomic write
//! (REQ-WSP-02) — on a background task, and clears the tab's dirty dot (REQ-UI-02) with
//! exactly the draft that landed: an edit typed while the write ran keeps the tab dirty.
//! A write that cannot land reports a toast naming the file and the reason, and so does a
//! request with no file in a workspace yet (AGENTS.md › errors reach the user).
//!
//! Where each request's file lives comes from the [`RequestFiles`] registry: the workspace
//! root and the request's [`Item`] placement in it, which is what
//! [`workspace_store::save_item`] needs to
//! keep a write on "its file" and what a failure toast names.
//!
//! `Decision:` the `workspace::Save` handler is registered app-level ([`init`] →
//! `App::on_action`), the shape [`crate::new_request`] uses for `workspace::NewRequest`,
//! rather than an `.on_action` on the editor's root element: this step's Edits keep
//! `request_editor.rs` to `mod`/init lines, so no line may join its `render`. Like
//! `new_request`'s `focus_url_bar`, the handler routes to the request tab that is active
//! in the active window — the tab the keystroke was pressed in — and says nothing when
//! the active tab is not a request (there is no request to save).
//!
//! `Decision:` the save task is detached, not stored on the editor: a write the user asked
//! for must land even if the tab closes while it runs, and dropping a `Task` would cancel
//! it. The completion takes the "editor gone" path explicitly, and the write is the
//! workspace store's atomic one either way.
//!
//! `Decision:` a successful save records the placement the store actually wrote back into
//! [`RequestFiles`] — a rename moves the file (REQ-WSP-03), and the next failure toast
//! must name where the request lives now.
//!
//! `Decision:` [`show_toast`] is this crate's copy of the deferred
//! `Workspace::show_toast` reach-through `response_viewer`'s body save runs: `Workspace`
//! exposes only the method itself, and promoting the shared reach-through is outside this
//! step's Edits. One idea, two small helpers, until a third wants it.

use std::path::{Path, PathBuf};

use api_model::{ItemId, Request};
use gpui::{AnyWindowHandle, App, Context, Global, Window};
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

/// Register the `workspace::Save` handler (see the module docs).
pub fn init(cx: &mut App) {
    cx.on_action(save_active_request);
}

/// `workspace::Save` (Ctrl-S / Cmd-S) for the request tab active in the active window.
/// Any other kind of tab is not a request to save and is left alone.
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
    /// Write this tab's draft to the request's file on a background task (REQ-WSP-01).
    /// The tab is marked clean with exactly the draft that landed, so an edit typed while
    /// the write ran keeps the tab dirty; a write that cannot land is reported to the
    /// user, never only logged.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let window_handle = window.window_handle();
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
        let saved = self.draft.clone();
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
                    if let Err(_released) = this.update(cx, |editor, cx| {
                        // Exactly the draft that landed: an edit typed while the write
                        // ran leaves the tab dirty against this one.
                        editor.saved = saved;
                        cx.notify();
                    }) {
                        // The tab closed while the write ran; its file holds the request
                        // all the same and the registry still records where.
                    }
                    cx.update(|cx| record_placement(&root, item, cx));
                }
                Err(error) => cx.update(|cx| {
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

/// Show `message` as a toast over the workspace shell hosting `window_handle`
/// (`Workspace::show_toast` is the one door in — the response body save reaches it the
/// same way, see the module docs). The update is deferred: a window cannot be updated
/// while it is on the update stack. A window whose root is not a [`Workspace`] (a bare
/// test window) has no toast layer, so there is nothing to show anything on.
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
                return;
            };
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message, level, None, cx);
            });
        });
        if shown.is_err() {
            // The window closed before the toast landed; its shell went with it.
        }
    });
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
