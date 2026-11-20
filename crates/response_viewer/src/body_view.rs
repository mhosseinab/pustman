//! The response body: Pretty, Raw, Hex and Preview (REQ-RESP-02) — what the pane draws
//! and how it got here.
//!
//! Raw shows the decoded text exactly, one line per line; only bytes that are not valid
//! UTF-8 decode lossily, and then with a visible note. Pretty shows the formatter's
//! output — JSON pretty-printed, XML and HTML indented by tag depth ([`crate::pretty`]) —
//! with the formatter picked from `Content-Type` and overridable by hand. Formatting is
//! heavy work, so it runs on a background task and the Raw lines stay on screen until the
//! formatted ones land.
//!
//! An in-memory body splits when it is set — one cheap pass, bounded by the spill limit.
//! A body that streamed or spilled to disk ([`http_engine::ResponseBody::File`],
//! [`http_engine::ResponseBody::Spilled`]) is read on a background task instead: the
//! foreground never touches the disk, and the pane says so until the lines land.
//!
//! Hex ([`crate::hex_view`]) dumps the body's raw bytes as `offset | hex | ascii` rows,
//! and Preview ([`crate::preview`]) shows images from those bytes and HTML as safe text.
//! Both draw from the same cached bytes the text modes draw from.

use std::borrow::Cow;
use std::sync::Arc;

use gpui::{
    Anchor, AnyElement, Entity, FocusHandle, Subscription, Task, Window, anchored, deferred,
};
use http_engine::{Header, Response, ResponseBody};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

use crate::child_id;
use crate::hex_view::{HexRows, HexView};
use crate::pretty::{FormatError, Formatter};
use crate::preview::{PreviewContent, PreviewPane};

/// The height of one formatter control (a [`ButtonSize::Medium`]) in pixels at the
/// default scale: the override menu is anchored just under it.
const FORMATTER_CONTROL_HEIGHT_PX: f32 = 28.;

/// The ways the body pane draws a body (REQ-RESP-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyMode {
    /// The formatter's output: JSON pretty-printed, XML and HTML indented.
    Pretty,
    /// The decoded text exactly, as it arrived.
    Raw,
    /// The raw bytes as a hexdump: `offset | hex | ascii`.
    Hex,
    /// Images from the body's own bytes; HTML as safe plain text.
    Preview,
}

impl BodyMode {
    /// Where the mode's tab sits on the mode bar.
    fn tab_index(self) -> usize {
        match self {
            Self::Pretty => 0,
            Self::Raw => 1,
            Self::Hex => 2,
            Self::Preview => 3,
        }
    }

    /// The mode a mode-bar tab stands for.
    fn from_tab_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Pretty),
            1 => Some(Self::Raw),
            2 => Some(Self::Hex),
            3 => Some(Self::Preview),
            _ => None,
        }
    }
}

/// The body pane: the lines it draws and how they got here.
pub struct BodyView {
    /// Raw mode's lines: the decoded text exactly, one entry per line.
    lines: Arc<Vec<SharedString>>,
    /// Pretty mode's lines, once the formatter landed off the foreground. Empty until
    /// then and after a format error — the pane falls back to the Raw lines either way.
    pretty_lines: Arc<Vec<SharedString>>,
    /// The decoded body text, cached so a mode or override change reformats without
    /// re-reading the body.
    text: Arc<str>,
    /// The body's raw bytes, cached the same way: Hex dumps them, Preview decodes or
    /// strips them.
    bytes: Arc<Vec<u8>>,
    /// The body's `Content-Type`, what Preview picks its rendering from.
    content_type: Option<Arc<str>>,
    /// The formatter `Content-Type` asked for; the override wins over it.
    detected: Formatter,
    /// The formatter picked by hand, if any.
    formatter_override: Option<Formatter>,
    mode: BodyMode,
    /// The formatter failed; the pane shows the Raw lines with this note instead.
    format_error: Option<SharedString>,
    /// What Preview draws for the body, computed off the foreground like the formatted
    /// lines. [`None`] until it lands.
    preview: Option<PreviewContent>,
    /// The bytes are not valid UTF-8, so the lines hold replacement characters.
    lossy: bool,
    /// A file-backed body is being read off the foreground.
    loading: bool,
    /// Reading a file-backed body failed; shown instead of the lines.
    error: Option<SharedString>,
    /// The in-flight disk read. Stored so replacing the body cancels it: a stale load
    /// must never overwrite a newer response (AGENTS.md › Async).
    load_task: Option<Task<()>>,
    /// The in-flight format, cancelled the same way by a newer body, mode or override.
    format_task: Option<Task<()>>,
    /// The in-flight preview computation, cancelled the same way by a newer body or mode.
    preview_task: Option<Task<()>>,
    /// The mode switch: one tab bar, keyboard-navigable like the viewer's tabs.
    mode_bar: Entity<TabBar>,
    /// The open formatter override menu and its subscription: dropped together when it
    /// dismisses (`request_editor::url_bar`).
    formatter_menu: Option<Entity<ContextMenu>>,
    formatter_menu_subscription: Option<Subscription>,
    /// The override trigger's focus handle — the menu hands focus back to it on close.
    formatter_focus_handle: FocusHandle,
    _mode_bar_subscription: Subscription,
}

impl BodyView {
    /// An empty pane, waiting for a response, showing bodies Pretty first (what Postman
    /// opens on).
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mode_bar = cx.new(|cx| TabBar::new(cx).with_tabs(mode_tabs(cx.entity_id())));
        let _mode_bar_subscription = cx.subscribe(&mode_bar, Self::on_mode_bar_event);
        Self {
            lines: Arc::new(Vec::new()),
            pretty_lines: Arc::new(Vec::new()),
            text: Arc::from(""),
            bytes: Arc::new(Vec::new()),
            content_type: None,
            detected: Formatter::Plain,
            formatter_override: None,
            mode: BodyMode::Pretty,
            format_error: None,
            preview: None,
            lossy: false,
            loading: false,
            error: None,
            load_task: None,
            format_task: None,
            preview_task: None,
            mode_bar,
            formatter_menu: None,
            formatter_menu_subscription: None,
            formatter_focus_handle: cx.focus_handle().tab_stop(true),
            _mode_bar_subscription,
        }
    }

    /// Show this response's body. Replacing the body drops the previous load and format
    /// tasks, which cancels them, so only the newest body can land on screen.
    pub fn set_body(&mut self, response: Arc<Response>, cx: &mut Context<Self>) {
        self.load_task = None;
        self.format_task = None;
        self.preview_task = None;
        self.error = None;
        self.lossy = false;
        self.loading = false;
        self.text = Arc::from("");
        self.bytes = Arc::new(Vec::new());
        self.lines = Arc::new(Vec::new());
        self.pretty_lines = Arc::new(Vec::new());
        self.preview = None;
        self.format_error = None;
        let content_type = content_type(&response.headers);
        self.detected = Formatter::detect(content_type);
        self.content_type = content_type.map(Arc::from);
        match &response.body {
            ResponseBody::Bytes(bytes) => {
                self.bytes = Arc::new(bytes.clone());
                let (text, lossy) = decode(bytes);
                self.lines = Arc::new(split_lines(&text));
                self.lossy = lossy;
                self.text = Arc::from(text.into_owned());
                self.refresh_format(cx);
                self.refresh_preview(cx);
            }
            ResponseBody::File { .. } | ResponseBody::Spilled { .. } => {
                self.loading = true;
                self.load_task = Some(cx.spawn(async move |this, cx| {
                    let loaded = cx
                        .background_spawn(async move {
                            let bytes = body_bytes(&response)?;
                            let (text, lossy) = decode(&bytes);
                            let text = Arc::<str>::from(text.into_owned());
                            let lines = split_lines(&text);
                            Ok::<_, std::io::Error>((Arc::new(bytes), text, lines, lossy))
                        })
                        .await;
                    let outcome = match loaded {
                        Ok(prepared) => Ok(prepared),
                        Err(error) => Err(error.to_string()),
                    };
                    if let Err(_released) = this.update(cx, |view, cx| {
                        view.loading = false;
                        match outcome {
                            Ok((bytes, text, lines, lossy)) => {
                                view.bytes = bytes;
                                view.text = text;
                                view.lines = Arc::new(lines);
                                view.lossy = lossy;
                                view.refresh_format(cx);
                                view.refresh_preview(cx);
                            }
                            Err(message) => {
                                view.error = Some(
                                    format!("Could not read the response body: {message}").into(),
                                );
                            }
                        }
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

    /// Show the body formatted (Pretty) or exactly as it arrived (Raw), and reformat the
    /// cached body when Pretty comes back on.
    pub fn set_mode(&mut self, mode: BodyMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.mode_bar.update(cx, |bar, cx| {
            if bar.selected() != mode.tab_index() {
                bar.set_selected(mode.tab_index(), cx);
            }
        });
        self.refresh_format(cx);
        self.refresh_preview(cx);
        cx.notify();
    }

    /// Override the detected formatter by hand — [`None`] returns to the `Content-Type`
    /// choice. The Pretty lines describe the old formatter, so they reformat.
    pub fn set_formatter_override(&mut self, formatter: Option<Formatter>, cx: &mut Context<Self>) {
        self.formatter_override = formatter;
        self.refresh_format(cx);
        cx.notify();
    }

    /// The Raw mode lines: the decoded text exactly, one entry per line.
    pub fn lines(&self) -> &[SharedString] {
        &self.lines
    }

    /// The lines the pane draws in the current mode: the formatter's output in Pretty
    /// mode (the Raw lines while it is still being computed, or after it failed), the
    /// exact text in Raw mode.
    pub fn shown_lines(&self) -> &[SharedString] {
        self.shown().as_slice()
    }

    /// Whether the body was not valid UTF-8 and the lines hold replacement characters.
    pub fn is_lossy(&self) -> bool {
        self.lossy
    }

    /// The hexdump rows Hex mode draws for the body on screen ([`crate::hex_view`]).
    pub fn hex_rows(&self) -> HexRows {
        HexRows::new(Arc::clone(&self.bytes))
    }

    /// What Preview shows for the body on screen, once it is computed
    /// ([`crate::preview`]).
    pub fn preview_content(&self) -> Option<&PreviewContent> {
        self.preview.as_ref()
    }

    /// What the pane draws: the formatted lines whenever Pretty has them, the Raw lines
    /// otherwise.
    fn shown(&self) -> &Arc<Vec<SharedString>> {
        if self.mode == BodyMode::Pretty && !self.pretty_lines.is_empty() {
            &self.pretty_lines
        } else {
            &self.lines
        }
    }

    /// The formatter in effect: the hand-picked override, or what `Content-Type` says.
    fn effective_formatter(&self) -> Formatter {
        self.formatter_override.unwrap_or(self.detected)
    }

    /// Reformat the cached body for the formatter in effect. Formatting is heavy work,
    /// so it runs on a background task: the Raw lines stay on screen until the formatted
    /// ones land, and a newer body, mode or override cancels the in-flight format.
    fn refresh_format(&mut self, cx: &mut Context<Self>) {
        self.format_task = None;
        self.pretty_lines = Arc::new(Vec::new());
        self.format_error = None;
        if self.mode != BodyMode::Pretty || self.text.is_empty() {
            return;
        }
        let text = Arc::clone(&self.text);
        let formatter = self.effective_formatter();
        self.format_task = Some(cx.spawn(async move |this, cx| {
            let formatted = cx
                .background_spawn(async move { formatter.format(text.as_bytes()) })
                .await;
            let outcome = match formatted {
                Ok(formatted) => Ok(split_lines(&formatted)),
                Err(error) => Err(format_error_note(formatter, error)),
            };
            if let Err(_released) = this.update(cx, |view, cx| {
                match outcome {
                    Ok(lines) => view.pretty_lines = Arc::new(lines),
                    Err(message) => view.format_error = Some(message),
                }
                cx.notify();
            }) {
                // Handled: the view went away with its window; there is nobody left to
                // show the formatted body to.
            }
        }));
    }

    /// Compute what Preview shows for the cached body. Reducing an HTML body to its text
    /// is heavy work, so it runs on a background task like the formatter — and a newer
    /// body or mode cancels the in-flight computation the same way.
    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_task = None;
        self.preview = None;
        if self.mode != BodyMode::Preview || self.bytes.is_empty() {
            return;
        }
        let bytes = Arc::clone(&self.bytes);
        let content_type = self.content_type.clone();
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            let content = cx
                .background_spawn(async move {
                    PreviewContent::for_body(content_type.as_deref(), bytes.as_slice())
                })
                .await;
            if let Err(_released) = this.update(cx, |view, cx| {
                view.preview = Some(content);
                cx.notify();
            }) {
                // Handled: the view went away with its window; there is nobody left to
                // show the preview to.
            }
        }));
    }

    /// The mode bar's events: moving the selection with the arrows or activating a tab
    /// with the mouse switches the mode.
    fn on_mode_bar_event(
        &mut self,
        mode_bar: Entity<TabBar>,
        event: &TabBarEvent,
        cx: &mut Context<Self>,
    ) {
        let index = match event {
            TabBarEvent::Selected { index } | TabBarEvent::Activated { index } => *index,
        };
        // The bar's current selection is the truth; an event that no longer describes it
        // is a stale echo and is dropped (`response_viewer::on_tab_bar_event`).
        if index != mode_bar.read(cx).selected() {
            return;
        }
        if let Some(mode) = BodyMode::from_tab_index(index) {
            self.set_mode(mode, cx);
        }
    }

    /// Open the formatter override menu under its trigger.
    fn open_formatter_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.formatter_menu.is_some() {
            return;
        }
        let picker = cx.weak_entity();
        let auto_picker = picker.clone();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx).entry(
                SharedString::new_static("Auto (from Content-Type)"),
                move |_event, _window, cx| {
                    if auto_picker
                        .update(cx, |view, cx| view.set_formatter_override(None, cx))
                        .is_err()
                    {
                        // The view was dropped while the menu was open; there is no
                        // body left to format.
                    }
                },
            );
            for formatter in [
                Formatter::Json,
                Formatter::Xml,
                Formatter::Html,
                Formatter::Plain,
            ] {
                let picker = picker.clone();
                menu = menu.entry(formatter.label(), move |_event, _window, cx| {
                    if picker
                        .update(cx, |view, cx| {
                            view.set_formatter_override(Some(formatter), cx)
                        })
                        .is_err()
                    {
                        // The view was dropped while the menu was open; there is no
                        // body left to format.
                    }
                });
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_formatter_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.formatter_menu = Some(menu);
        self.formatter_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The override menu is done — an entry ran or Escape backed out — so it goes away
    /// and focus returns to its trigger.
    fn on_formatter_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.formatter_menu = None;
        self.formatter_menu_subscription = None;
        window.focus(&self.formatter_focus_handle, cx);
        cx.notify();
    }

    /// The mode switch and the formatter override above the lines.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .px(DynamicSpacing::Base04.rems(cx))
            .child(self.mode_bar.clone())
            .child(self.render_formatter_control(cx))
    }

    /// The override trigger — a dropdown button, drawn like the URL bar's method picker
    /// (`request_editor::url_bar`) — and its floating menu while it is open.
    fn render_formatter_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let label = match self.formatter_override {
            Some(formatter) => SharedString::from(formatter.label()),
            None => SharedString::new_static("Auto"),
        };
        let control = Button::new(ElementId::from(("body-formatter", cx.entity_id())), label)
            .size(ButtonSize::Medium)
            .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small))
            .tooltip(Tooltip::text("Choose the body formatter"))
            .focus_handle(self.formatter_focus_handle.clone())
            .on_click(cx.listener(|this, _event, window, cx| this.open_formatter_menu(window, cx)))
            .into_any_element();
        div()
            .relative()
            .child(control)
            .children(self.formatter_menu.clone().map(|menu| {
                div()
                    .absolute()
                    .top(px_to_rems(FORMATTER_CONTROL_HEIGHT_PX))
                    .right_0()
                    .child(
                        deferred(
                            anchored()
                                .anchor(Anchor::TopLeft)
                                .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                                .child(menu),
                        )
                        .with_priority(1),
                    )
            }))
            .into_any_element()
    }

    /// Pretty and Raw: the lines of the mode in effect, or the pane's state when there
    /// are none to draw.
    fn render_lines(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        let lines = Arc::clone(self.shown());
        if lines.is_empty() && !self.loading && self.error.is_none() {
            return note("No content", cx).into_any_element();
        }
        if lines.is_empty() {
            return div().into_any_element();
        }
        let row_lines = Arc::clone(&lines);
        let row_base = base.clone();
        let list_base = base.clone();
        let render_base = base.clone();
        VirtualList::new(
            child_id(&list_base, "lines"),
            lines.len(),
            move |index| child_id(&row_base, format!("line-{index}")),
            move |index, _window, cx| render_line(&row_lines, index, &render_base, cx),
        )
        .into_any_element()
    }

    /// Hex: the body's raw bytes as hexdump rows ([`crate::hex_view`]), formatted lazily
    /// per visible row.
    fn render_hex(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        if self.bytes.is_empty() && !self.loading {
            return note("No content", cx).into_any_element();
        }
        HexView::new(base.clone(), Arc::clone(&self.bytes)).into_any_element()
    }

    /// Preview: what [`crate::preview`] made of the body, or a note while it computes.
    fn render_preview(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        if self.bytes.is_empty() && !self.loading {
            return note("No content", cx).into_any_element();
        }
        match &self.preview {
            Some(content) => PreviewPane::new(base.clone(), content.clone()).into_any_element(),
            None => note("Preparing preview…", cx).into_any_element(),
        }
    }
}

impl Render for BodyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("body-view", cx.entity_id()));
        v_flex()
            .size_full()
            .child(self.render_toolbar(cx))
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
            .when(
                self.lossy && matches!(self.mode, BodyMode::Pretty | BodyMode::Raw),
                |this| {
                    this.child(note(
                        "Body is not valid UTF-8 — shown with replacement characters.",
                        cx,
                    ))
                },
            )
            .map(|this| match &self.format_error {
                Some(message) => this.child(note(message.clone(), cx)),
                None => this,
            })
            .map(|this| match self.mode {
                BodyMode::Pretty | BodyMode::Raw => this.child(self.render_lines(&base, cx)),
                BodyMode::Hex => this.child(self.render_hex(&base, cx)),
                BodyMode::Preview => this.child(self.render_preview(&base, cx)),
            })
    }
}

/// One body line: the content font, truncated with the full line behind a tooltip.
/// Shared with the Preview text lines ([`crate::preview`]): a body pane's lines all draw
/// the same way.
pub(crate) fn render_line(
    lines: &[SharedString],
    index: usize,
    base: &ElementId,
    cx: &mut App,
) -> AnyElement {
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

/// A muted note line — the empty, loading, lossy and format-fallback states.
pub(crate) fn note(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .child(Label::new(text.into()).color(Color::Muted))
}

/// The note a failed format leaves behind: what was expected, and that Raw is showing.
fn format_error_note(formatter: Formatter, error: FormatError) -> SharedString {
    format!(
        "Could not format the body as {}: {error} — shown as received.",
        formatter.label()
    )
    .into()
}

/// The mode switch's tabs, one per [`BodyMode`] under a stable base id.
fn mode_tabs(view_id: gpui::EntityId) -> Vec<Tab> {
    let base = ElementId::from(("body-mode", view_id));
    [
        BodyMode::Pretty,
        BodyMode::Raw,
        BodyMode::Hex,
        BodyMode::Preview,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, mode)| Tab::new(child_id(&base, format!("mode-{index}")), mode_label(mode)))
    .collect()
}

/// The name of a mode's tab on the switch.
fn mode_label(mode: BodyMode) -> &'static str {
    match mode {
        BodyMode::Pretty => "Pretty",
        BodyMode::Raw => "Raw",
        BodyMode::Hex => "Hex",
        BodyMode::Preview => "Preview",
    }
}

/// The body's `Content-Type` header value — what the formatter is detected from.
fn content_type(headers: &[Header]) -> Option<&str> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str())
}

/// The body as text: exactly, or lossily with `lossy` set when the bytes are not UTF-8.
fn decode(bytes: &[u8]) -> (Cow<'_, str>, bool) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (Cow::Borrowed(text), false),
        Err(_) => (String::from_utf8_lossy(bytes), true),
    }
}

/// Split text into the lines the pane draws. A trailing newline ends the last line
/// instead of adding an empty one.
fn split_lines(text: &str) -> Vec<SharedString> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<SharedString> = text.split('\n').map(SharedString::from).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
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
