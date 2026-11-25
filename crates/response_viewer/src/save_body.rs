//! Saving and copying the response body (REQ-RESP-07).
//!
//! `response::SaveBody` asks where the body goes with the platform save dialog and
//! writes its bytes on a background task; `response::CopyBody` puts the body text on the
//! clipboard. A failed write lands in an error toast naming the path and the OS error —
//! never only a log (AGENTS.md › Errors must reach the user). Body bytes never reach a
//! log, toast or element id (AGENTS.md › Security).
//!
//! The handlers live on [`ResponseView`] — the view that owns the body — and every task
//! they start is stored on it, so a second save or copy replaces (cancels) the first
//! (AGENTS.md › Async).
//!
//! `Decision:` a save writes the bytes the response arrived as — byte-exact for binary
//! bodies — while [`ResponseView::copy_body`] copies the text the Raw pane shows,
//! decoding non-text bytes lossily, because a clipboard only carries text.
//!
//! `Decision:` [`write_body`] is a plain write, not the atomic temp-file write
//! `workspace_store` does for its own items: that helper is private there and this file
//! is one the user just chose. Promote it to a shared atomic writer if saves ever need
//! crash-safe replaces.
//!
//! `Decision:` the save dialog opens at the process's working directory and suggests
//! `SUGGESTED_NAME` — this layer has no request name to derive one from.
//!
//! `Decision:` the body's bytes come from one extraction, `body_view::body_bytes` — it
//! was duplicated here while `body_view.rs` belonged to a parallel step (S29b), and the
//! two are one again (#145). Both call sites run it on a background task (AGENTS.md ›
//! Async).

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::{AnyWindowHandle, ClipboardItem};
use pustman_actions::response::{CopyBody, SaveBody};
use ui::prelude::*;
use workspace::Workspace;

use crate::ResponseView;
use crate::body_view::body_bytes;

/// The name the save dialog starts with (see the module docs).
const SUGGESTED_NAME: &str = "response-body.txt";

/// What to say when the user asks to save or copy before any body arrived.
const NO_BODY_TO_SAVE: &str = "There is no response body to save yet.";
const NO_BODY_TO_COPY: &str = "There is no response body to copy yet.";

impl ResponseView {
    /// `response::SaveBody` (REQ-RESP-07): pick a file, then write the body's bytes off
    /// the foreground. A dialog that never answered or a write that failed says so in an
    /// error toast; a dialog the user dismissed is their decision, not a failure.
    pub(crate) fn save_body(&mut self, _: &SaveBody, window: &mut Window, cx: &mut Context<Self>) {
        let window_handle = window.window_handle();
        let Some(response) = self.response.clone() else {
            show_toast(window_handle, NO_BODY_TO_SAVE, Severity::Warning, cx);
            return;
        };
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let chosen = cx.prompt_for_new_path(&directory, Some(SUGGESTED_NAME));
        self.save_task = Some(cx.spawn(async move |_this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    cx.update(|cx| {
                        show_toast(
                            window_handle,
                            format!("Could not open the save dialog — {error}"),
                            Severity::Error,
                            cx,
                        );
                    });
                    return;
                }
                Err(_canceled) => {
                    cx.update(|cx| {
                        show_toast(
                            window_handle,
                            "The save dialog did not return a file name",
                            Severity::Error,
                            cx,
                        );
                    });
                    return;
                }
            };
            let written = cx
                .background_spawn(async move {
                    let bytes = body_bytes(&response)?;
                    write_body(&path, &bytes)
                })
                .await;
            if let Err(error) = written {
                cx.update(|cx| {
                    show_toast(
                        window_handle,
                        format!("Could not save the response body — {error:#}"),
                        Severity::Error,
                        cx,
                    );
                });
            }
        }));
    }

    /// `response::CopyBody` (REQ-RESP-07): the body text as the Raw pane shows it — the
    /// decoded text, non-text bytes lossily — on the clipboard.
    pub(crate) fn copy_body(&mut self, _: &CopyBody, window: &mut Window, cx: &mut Context<Self>) {
        let window_handle = window.window_handle();
        let Some(response) = self.response.clone() else {
            show_toast(window_handle, NO_BODY_TO_COPY, Severity::Warning, cx);
            return;
        };
        self.copy_task = Some(cx.spawn(async move |_this, cx| {
            let copied = cx
                .background_spawn(async move {
                    let bytes = body_bytes(&response)?;
                    Ok::<String, std::io::Error>(String::from_utf8_lossy(&bytes).into_owned())
                })
                .await;
            match copied {
                Ok(text) => cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text))),
                Err(error) => cx.update(|cx| {
                    show_toast(
                        window_handle,
                        format!("Could not copy the response body — {error}"),
                        Severity::Error,
                        cx,
                    );
                }),
            }
        }));
    }
}

/// Write `body`'s exact bytes to `path`. Pure on purpose: the dialog, the background
/// task and the error reporting live in [`ResponseView::save_body`], and this is the one
/// part of the save a test can pin down (REQ-RESP-07).
pub fn write_body(path: &Path, body: &[u8]) -> anyhow::Result<()> {
    std::fs::write(path, body).with_context(|| format!("could not write {}", path.display()))
}

/// Show `message` as a toast over the workspace shell hosting `window_handle`
/// (`Workspace::show_toast` is the one door in — the environment quick-look's opener
/// reaches it the same way). The update is deferred: a window cannot be updated while it
/// is on the update stack. A window whose root is not a [`Workspace`] (a bare test
/// window) has no toast layer, so there is nothing to show anything on.
pub(crate) fn show_toast(
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
#[path = "save_body_tests.rs"]
mod tests;
