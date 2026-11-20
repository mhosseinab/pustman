//! The response body in Raw mode (REQ-RESP-02): the bytes as they arrived, split into
//! lines and drawn through [`VirtualList`].
//!
//! An in-memory body splits when it is set — one cheap pass, bounded by the spill limit.
//! A body that streamed or spilled to disk ([`http_engine::ResponseBody::File`],
//! [`http_engine::ResponseBody::Spilled`]) is read on a background task instead: the
//! foreground never touches the disk, and the pane says so until the lines land. The
//! text is shown exactly; only bytes that are not valid UTF-8 decode lossily, and then
//! with a visible note.

use std::borrow::Cow;
use std::sync::Arc;

use gpui::Task;
use http_engine::{Response, ResponseBody};
use ui::prelude::*;

use crate::child_id;

/// The Raw body pane: the lines it draws and how they got here.
pub struct BodyView {
    lines: Arc<Vec<SharedString>>,
    /// The bytes are not valid UTF-8, so the lines hold replacement characters.
    lossy: bool,
    /// A file-backed body is being read off the foreground.
    loading: bool,
    /// Reading a file-backed body failed; shown instead of the lines.
    error: Option<SharedString>,
    /// The in-flight disk read. Stored so replacing the body cancels it: a stale load
    /// must never overwrite a newer response (AGENTS.md › Async).
    load_task: Option<Task<()>>,
}

impl BodyView {
    /// An empty pane, waiting for a response.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            lines: Arc::new(Vec::new()),
            lossy: false,
            loading: false,
            error: None,
            load_task: None,
        }
    }

    /// Show this response's body in Raw mode. Replacing the body drops the previous load
    /// task, which cancels it, so only the newest body can land on screen.
    pub fn set_body(&mut self, response: Arc<Response>, cx: &mut Context<Self>) {
        self.load_task = None;
        self.error = None;
        self.lossy = false;
        self.loading = false;
        match &response.body {
            ResponseBody::Bytes(bytes) => {
                let (lines, lossy) = raw_lines(bytes);
                self.lines = Arc::new(lines);
                self.lossy = lossy;
            }
            ResponseBody::File { .. } | ResponseBody::Spilled { .. } => {
                self.lines = Arc::new(Vec::new());
                self.loading = true;
                self.load_task = Some(cx.spawn(async move |this, cx| {
                    let loaded = cx
                        .background_spawn(async move { body_bytes(&response) })
                        .await;
                    let outcome = match loaded {
                        Ok(bytes) => Ok(raw_lines(&bytes)),
                        Err(error) => Err(error.to_string()),
                    };
                    if let Err(_released) = this.update(cx, |view, cx| {
                        match outcome {
                            Ok((lines, lossy)) => {
                                view.lines = Arc::new(lines);
                                view.lossy = lossy;
                            }
                            Err(message) => {
                                view.error = Some(
                                    format!("Could not read the response body: {message}").into(),
                                );
                            }
                        }
                        view.loading = false;
                        cx.notify();
                    }) {
                        // Handled: the view went away with its window; there is nobody
                        // left to show the body to.
                    }
                }));
            }
        }
        cx.notify();
    }

    /// The Raw mode lines: the decoded text exactly, one entry per line.
    pub fn lines(&self) -> &[SharedString] {
        &self.lines
    }

    /// Whether the body was not valid UTF-8 and the lines hold replacement characters.
    pub fn is_lossy(&self) -> bool {
        self.lossy
    }
}

impl Render for BodyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = Arc::clone(&self.lines);
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("body-view", cx.entity_id()));
        v_flex()
            .size_full()
            .map(|this| match &self.error {
                Some(message) => this.child(
                    div()
                        .px(DynamicSpacing::Base04.rems(cx))
                        .py(DynamicSpacing::Base02.rems(cx))
                        .child(Label::new(message.clone()).color(Color::Error)),
                ),
                None => this,
            })
            .when(self.loading, |this| this.child(note("Loading body…", cx)))
            .when(self.lossy, |this| {
                this.child(note(
                    "Body is not valid UTF-8 — shown with replacement characters.",
                    cx,
                ))
            })
            .map(|this| {
                if lines.is_empty() && !self.loading && self.error.is_none() {
                    this.child(note("No content", cx))
                } else if lines.is_empty() {
                    this
                } else {
                    let row_lines = Arc::clone(&lines);
                    let row_base = base.clone();
                    this.child(VirtualList::new(
                        child_id(&base, "lines"),
                        lines.len(),
                        move |index| child_id(&row_base, format!("line-{index}")),
                        move |index, _window, cx| render_line(&row_lines, index, &base, cx),
                    ))
                }
            })
    }
}

/// One body line: the content font, truncated with the full line behind a tooltip.
fn render_line(lines: &[SharedString], index: usize, base: &ElementId, cx: &mut App) -> AnyElement {
    let Some(line) = lines.get(index).cloned() else {
        return div().into_any_element();
    };
    let line_id = child_id(base, format!("line-{index}"));
    div()
        .id(child_id(&line_id, "text"))
        .w_full()
        .min_w_0()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base01.rems(cx))
        .font_buffer(cx)
        .text_buffer(cx)
        .text_color(cx.theme().colors().text)
        .tooltip(Tooltip::text(line.clone()).builder())
        .child(div().w_full().min_w_0().truncate().child(line))
        .into_any_element()
}

/// A muted note line — the empty, loading and lossy states.
fn note(text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .child(Label::new(text).color(Color::Muted))
}

/// Split `bytes` into the lines Raw mode draws. The text is shown exactly — only bytes
/// that are not valid UTF-8 decode lossily, and `lossy` says so the view can show the
/// note. A trailing newline ends the last line instead of adding an empty one.
fn raw_lines(bytes: &[u8]) -> (Vec<SharedString>, bool) {
    let (text, lossy) = match std::str::from_utf8(bytes) {
        Ok(text) => (Cow::Borrowed(text), false),
        Err(_) => (String::from_utf8_lossy(bytes), true),
    };
    if text.is_empty() {
        return (Vec::new(), lossy);
    }
    let mut lines: Vec<SharedString> = text.split('\n').map(SharedString::from).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    (lines, lossy)
}

/// The whole body's bytes, read off the foreground: the file behind a streamed or spilled
/// body, with the spilled prefix kept in front of the remainder.
fn body_bytes(response: &Response) -> std::io::Result<Vec<u8>> {
    match &response.body {
        ResponseBody::Bytes(bytes) => Ok(bytes.clone()),
        ResponseBody::File { path, .. } => std::fs::read(path),
        ResponseBody::Spilled { prefix, file, .. } => {
            let remainder = std::fs::read(file.path())?;
            let mut bytes = Vec::with_capacity(prefix.len() + remainder.len());
            bytes.extend_from_slice(prefix);
            bytes.extend_from_slice(&remainder);
            Ok(bytes)
        }
    }
}

#[cfg(test)]
#[path = "body_view_tests.rs"]
mod tests;
