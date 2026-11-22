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
//! foreground never touches the disk, and the pane says so until the lines land. A body
//! over `request.response_pretty_limit_mb` opens Raw with a banner instead of being
//! pretty-printed ([`crate::large_body`], REQ-RESP-06).
//!
//! Hex ([`crate::hex_view`]) dumps the body's raw bytes as `offset | hex | ascii` rows,
//! and Preview ([`crate::preview`]) shows images from those bytes and HTML as safe text.
//! Both draw from the same cached bytes the text modes draw from.

use std::borrow::Cow;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Anchor, AnyElement, Bounds, ClipboardItem, DispatchPhase, Entity, FocusHandle, Focusable,
    HighlightStyle, InteractiveText, KeyBinding, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollStrategy, StyledText, Subscription, Task, TextLayout, UniformListScrollHandle,
    WeakEntity, Window, anchored, canvas, deferred, point,
};
use http_engine::{Header, Response, ResponseBody};
use settings::Setting;
use text_input::buffer::word_range_at;
use text_input::input::{Copy, SelectAll};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

use crate::body_selection::{BodyPoint, BodySelection, clamp_column};
use crate::child_id;
use crate::find::{FindBar, FindEvent, FindMatch};
use crate::hex_view::{HexRows, HexView};
use crate::json_copy::{js_string_literal, json_string_literal, json_target_at, property_path};
use crate::json_syntax::{JsonToken, LineSpans, json_line_spans, layer_highlights};
use crate::large_body::{self, LargeBodyBanner, LargeBodyNotice, ResponseViewerSetting};
use crate::pretty::{FormatError, Formatter};
use crate::preview::{PreviewContent, PreviewPane};
use crate::status_line::body_size;

/// The height of one formatter control (a [`ButtonSize::Medium`]) in pixels at the
/// default scale: the override menu is anchored just under it.
const FORMATTER_CONTROL_HEIGHT_PX: f32 = 28.;

/// The key context of the body pane: copy and select-all act on the hand-made selection
/// over its lines.
pub(crate) const BODY_CONTEXT: &str = "ResponseBody";

/// Copy and select-all over the body's lines, on each platform's usual keys.
pub(crate) fn init(cx: &mut App) {
    let (copy, select_all) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-a")
    } else {
        ("ctrl-c", "ctrl-a")
    };
    cx.bind_keys([
        KeyBinding::new(copy, Copy, Some(BODY_CONTEXT)),
        KeyBinding::new(select_all, SelectAll, Some(BODY_CONTEXT)),
    ]);
}

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
    /// The JSON syntax spans of [`Self::pretty_lines`], one list per line, computed with
    /// them. Empty when Pretty did not format the body as JSON.
    syntax_spans: Arc<Vec<LineSpans>>,
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
    /// The large-body fallback in effect (REQ-RESP-06, [`crate::large_body`]): the body
    /// is over the pretty limit, so Raw shows and the banner above the lines names the
    /// limit and offers the body's file.
    large_body: Option<LargeBodyNotice>,
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
    /// The find bar over the lines while it is open (REQ-RESP-03), and the subscription
    /// bringing its events back here.
    find_bar: Option<Entity<FindBar>>,
    find_subscription: Option<Subscription>,
    /// The lines list's scroll state: the find bar scrolls the current match into view
    /// through it.
    list_scroll_handle: UniformListScrollHandle,
    /// The pane's own focus handle — what Escape from the find bar hands focus back to.
    focus_handle: FocusHandle,
    /// The text selected by hand over the Pretty or Raw lines, and whether a drag is
    /// extending it right now.
    selection: Option<BodySelection>,
    selecting: bool,
    /// What the press that started the selection selects by, so a drag grows by it.
    select_unit: SelectUnit,
    /// Where a selecting drag that left the rows is, with the rows' bounds; the
    /// autoscroll keeps stepping toward it while it stays past an edge.
    drag_outside: Option<(Point<Pixels>, Bounds<Pixels>)>,
    autoscroll: Option<Task<()>>,
    /// The right-click copy menu over the lines, where it opened (window coordinates),
    /// and its subscription: dropped together when it dismisses.
    body_menu: Option<(Entity<ContextMenu>, Point<Pixels>)>,
    body_menu_subscription: Option<Subscription>,
    _mode_bar_subscription: Subscription,
}

impl BodyView {
    /// An empty pane, waiting for a response, showing bodies Pretty first (what Postman
    /// opens on).
    pub fn new(cx: &mut Context<Self>) -> Self {
        // The pane reads `request.response_pretty_limit_mb` (REQ-RESP-06); registering
        // here makes the read safe however the host orders its settings init.
        ResponseViewerSetting::register(cx);
        let mode_bar = cx.new(|cx| {
            TabBar::new(cx)
                .segmented()
                .with_tabs(mode_tabs(cx.entity_id()))
        });
        let _mode_bar_subscription = cx.subscribe(&mode_bar, Self::on_mode_bar_event);
        Self {
            lines: Arc::new(Vec::new()),
            pretty_lines: Arc::new(Vec::new()),
            syntax_spans: Arc::new(Vec::new()),
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
            large_body: None,
            load_task: None,
            format_task: None,
            preview_task: None,
            mode_bar,
            formatter_menu: None,
            formatter_menu_subscription: None,
            formatter_focus_handle: cx.focus_handle().tab_stop(true),
            find_bar: None,
            find_subscription: None,
            list_scroll_handle: UniformListScrollHandle::default(),
            focus_handle: cx.focus_handle(),
            selection: None,
            selecting: false,
            select_unit: SelectUnit::Character,
            drag_outside: None,
            autoscroll: None,
            body_menu: None,
            body_menu_subscription: None,
            _mode_bar_subscription,
        }
    }

    /// The selection, while the lines it was made on are the ones on screen: a new body,
    /// another mode or the formatted lines landing all leave it behind.
    fn active_selection(&self) -> Option<&BodySelection> {
        self.selection.as_ref().filter(|selection| {
            !selection.is_empty()
                && matches!(self.mode, BodyMode::Pretty | BodyMode::Raw)
                && Arc::ptr_eq(&selection.lines, self.shown())
        })
    }

    /// The text selected by hand, if any.
    pub(crate) fn selected_text(&self) -> Option<String> {
        self.active_selection().map(BodySelection::text)
    }

    #[cfg(test)]
    pub(crate) fn selection_head_line(&self) -> Option<usize> {
        self.selection.as_ref().map(|selection| selection.head.line)
    }

    /// A press on a line, as editors take one: a single click places the start, a double
    /// click selects the word, a triple click the line with its line break, and more
    /// select everything. With Shift it moves the end of the current selection there.
    fn begin_selection(
        &mut self,
        at: BodyPoint,
        extend: bool,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines = Arc::clone(self.shown());
        match self.selection.as_mut() {
            Some(selection) if extend && Arc::ptr_eq(&selection.lines, &lines) => {
                selection.head = at;
                self.select_unit = SelectUnit::Character;
            }
            _ => {
                self.select_unit = match click_count {
                    0 | 1 => SelectUnit::Character,
                    2 => SelectUnit::Word(word_at(&lines, at)),
                    3 => SelectUnit::Line(line_at(&lines, at)),
                    _ => SelectUnit::All,
                };
                self.selection = Some(match &self.select_unit {
                    SelectUnit::Character => BodySelection {
                        lines,
                        anchor: at,
                        head: at,
                    },
                    SelectUnit::Word((start, end)) | SelectUnit::Line((start, end)) => {
                        BodySelection {
                            lines,
                            anchor: *start,
                            head: *end,
                        }
                    }
                    SelectUnit::All => BodySelection::all(lines),
                });
            }
        }
        self.selecting = true;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A drag to `at`: move the selection's end there, or — after a double or triple
    /// click — select from the pressed word or line through the one under the pointer.
    fn extend_selection(&mut self, at: BodyPoint, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        let Some(selection) = self.selection.as_mut() else {
            return;
        };
        let (anchor, head) = match &self.select_unit {
            SelectUnit::Character => (selection.anchor, at),
            SelectUnit::All => return,
            SelectUnit::Word(pressed) | SelectUnit::Line(pressed) => {
                let (start, end) = *pressed;
                let (under_start, under_end) = match self.select_unit {
                    SelectUnit::Word(_) => word_at(&selection.lines, at),
                    _ => line_at(&selection.lines, at),
                };
                if under_start < start {
                    (end, under_start)
                } else {
                    (start, end.max(under_end))
                }
            }
        };
        if selection.anchor != anchor || selection.head != head {
            selection.anchor = anchor;
            selection.head = head;
            cx.notify();
        }
    }

    fn end_selection(&mut self) {
        self.selecting = false;
        self.drag_outside = None;
        self.autoscroll = None;
    }

    /// A selecting drag anywhere in the window, seen against the rows' `bounds`. Over a
    /// row the row itself extends the selection; below the last row the selection runs
    /// to the end of the body, and past the top or bottom edge the rows scroll toward
    /// the pointer — and keep scrolling while it is held there.
    fn drag_beyond_rows(
        &mut self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.selecting {
            return;
        }
        if position.y < bounds.top() || position.y > bounds.bottom() {
            let was_inside = self.drag_outside.is_none();
            self.drag_outside = Some((position, bounds));
            self.autoscroll_step(cx);
            if was_inside {
                self.autoscroll = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(AUTOSCROLL_INTERVAL).await;
                        let stepping = this
                            .update(cx, |this, cx| this.autoscroll_step(cx))
                            .unwrap_or(false);
                        if !stepping {
                            break;
                        }
                    }
                }));
            }
            return;
        }
        self.drag_outside = None;
        self.autoscroll = None;
        let Some((row_height, scroll_top)) = self.row_geometry() else {
            return;
        };
        let lines = Arc::clone(self.shown());
        let below_rows = bounds.top() + row_height * lines.len() as f32 - scroll_top;
        if position.y >= below_rows {
            self.extend_selection(end_of_line(&lines, lines.len().saturating_sub(1)), cx);
        }
    }

    /// One autoscroll step toward a pointer held past an edge: scroll by a few rows —
    /// more the farther out it is — and move the selection to the row at that edge.
    /// False once the drag ended or came back over the rows.
    fn autoscroll_step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((position, bounds)) = self.drag_outside.filter(|_| self.selecting) else {
            return false;
        };
        let Some((row_height, scroll_top)) = self.row_geometry() else {
            return true;
        };
        let lines = Arc::clone(self.shown());
        let Some(last) = lines.len().checked_sub(1) else {
            return false;
        };
        let distance = if position.y < bounds.top() {
            position.y - bounds.top()
        } else {
            position.y - bounds.bottom()
        };
        let rows = (distance.abs() / row_height)
            .ceil()
            .clamp(1., AUTOSCROLL_MAX_ROWS);
        let handle = self.list_scroll_handle.0.borrow().base_handle.clone();
        let max_scroll = handle.max_offset().y;
        let scroll_top =
            (scroll_top + row_height * rows * distance.signum()).clamp(px(0.), max_scroll);
        handle.set_offset(point(handle.offset().x, -scroll_top));
        let at = if distance < px(0.) {
            let row = (scroll_top / row_height).floor() as usize;
            BodyPoint {
                line: row.min(last),
                column: 0,
            }
        } else {
            let row = ((scroll_top + bounds.size.height) / row_height).floor() as usize;
            end_of_line(&lines, row.min(last))
        };
        self.extend_selection(at, cx);
        cx.notify();
        true
    }

    /// The rows' height and how far they are scrolled down, once the list has laid out.
    /// The list records its whole content's size, and its rows are uniform.
    fn row_geometry(&self) -> Option<(Pixels, Pixels)> {
        let state = self.list_scroll_handle.0.borrow();
        let rows = self.shown().len();
        let row_height = state.last_item_size?.contents.height / rows.max(1) as f32;
        (row_height > px(0.)).then(|| (row_height, -state.base_handle.offset().y))
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.mode, BodyMode::Pretty | BodyMode::Raw) {
            return;
        }
        self.selection = Some(BodySelection::all(Arc::clone(self.shown())));
        cx.notify();
    }

    fn copy_selection(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// What the right-click menu offers at `at`: the selection, and on a Pretty JSON line
    /// the clicked value, its property path and — for a key or string — the string in the
    /// forms a developer pastes (the browser DevTools set).
    fn body_menu_entries(&self, at: BodyPoint) -> Vec<(&'static str, String)> {
        let mut entries = Vec::new();
        if let Some(text) = self.selected_text() {
            entries.push(("Copy", text));
        }
        let showing_json = self.mode == BodyMode::Pretty
            && Arc::ptr_eq(self.shown(), &self.pretty_lines)
            && !self.syntax_spans.is_empty();
        let target = showing_json
            .then(|| json_target_at(&self.pretty_lines, &self.syntax_spans, at))
            .flatten();
        if let Some(target) = target {
            entries.push(("Copy value", target.value));
            if !target.path.is_empty() {
                entries.push(("Copy property path", property_path(&target.path)));
            }
            if let Some(string) = target.string {
                let javascript = js_string_literal(&string);
                let json = json_string_literal(&string);
                entries.push(("Copy string contents", string));
                entries.push(("Copy string as JavaScript literal", javascript));
                entries.push(("Copy string as JSON literal", json));
            }
        }
        entries
    }

    /// Open the copy menu for a right-click at `at`, drawn at `position`. A click with
    /// nothing to copy opens nothing.
    fn open_body_menu(
        &mut self,
        at: BodyPoint,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.body_menu_entries(at);
        if entries.is_empty() {
            return;
        }
        let menu = cx.new(|cx| {
            entries
                .into_iter()
                .fold(ContextMenu::new(cx), |menu, (label, text)| {
                    menu.entry(
                        SharedString::new_static(label),
                        move |_event, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    )
                })
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_body_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.body_menu = Some((menu, position));
        self.body_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The copy menu is done — an entry ran or Escape backed out — so it goes away and
    /// focus returns to the pane, where the copy keystroke still works.
    fn on_body_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.body_menu = None;
        self.body_menu_subscription = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Show this response's body. Replacing the body drops the previous load and format
    /// tasks, which cancels them, so only the newest body can land on screen.
    pub fn set_body(&mut self, response: Arc<Response>, cx: &mut Context<Self>) {
        self.load_task = None;
        self.format_task = None;
        self.preview_task = None;
        self.error = None;
        self.large_body = None;
        self.lossy = false;
        self.loading = false;
        self.text = Arc::from("");
        self.bytes = Arc::new(Vec::new());
        self.lines = Arc::new(Vec::new());
        self.pretty_lines = Arc::new(Vec::new());
        self.syntax_spans = Arc::new(Vec::new());
        self.preview = None;
        self.format_error = None;
        let content_type = content_type(&response.headers);
        self.detected = Formatter::detect(content_type);
        self.content_type = content_type.map(Arc::from);
        // The large-body fallback (REQ-RESP-06) decides before anything formats: a
        // body whose size is already known goes to Raw here, a `File` body once its
        // read has measured it (the load below).
        let limit_mb = large_body::pretty_limit_mb(cx);
        let file_path = large_body::file_path(&response.body);
        self.apply_large_body_fallback(body_size(&response.body), limit_mb, file_path.clone(), cx);
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
                                // A `File` body's size is only known once it is read:
                                // the same fallback applies here, before any formatter
                                // is handed the huge body.
                                view.apply_large_body_fallback(
                                    u64::try_from(bytes.len()).ok(),
                                    limit_mb,
                                    file_path,
                                    cx,
                                );
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
                        view.refresh_find(cx);
                        cx.notify();
                    }) {
                        // Handled: the view went away with its window; there is nobody
                        // left to show the body to.
                    }
                }));
            }
        }
        self.refresh_find(cx);
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
        self.refresh_find(cx);
        cx.notify();
    }

    /// The large-body fallback (REQ-RESP-06): when `size` says the body is over
    /// `limit_mb`, the pane shows Raw instead of formatting a huge body and keeps the
    /// banner ([`LargeBodyNotice`]) above the lines. It applies once per body — a mode
    /// the user picks afterwards stands — and a size that cannot be measured keeps the
    /// normal flow.
    fn apply_large_body_fallback(
        &mut self,
        size: Option<u64>,
        limit_mb: u64,
        file_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.large_body.is_some() {
            return;
        }
        let Some(size) = size else {
            return;
        };
        if !large_body::exceeds_pretty_limit(size, limit_mb) {
            return;
        }
        self.large_body = Some(LargeBodyNotice::new(limit_mb, file_path));
        self.set_mode(BodyMode::Raw, cx);
    }

    /// Reveal the large body's file in the system file manager (REQ-RESP-06): the
    /// banner button's whole job. The path stays out of logs and element ids (PRD §7).
    fn reveal_large_body_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .large_body
            .as_ref()
            .and_then(LargeBodyNotice::file_path)
        else {
            return;
        };
        cx.reveal_path(path);
    }

    /// Open the find bar over the lines (REQ-RESP-03). It is created on first use and
    /// re-synced and focused on every `response::Find`.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_bar.is_none() {
            let bar = cx.new(|cx| FindBar::new(window, cx));
            let subscription = cx.subscribe_in(&bar, window, Self::on_find_event);
            self.find_bar = Some(bar);
            self.find_subscription = Some(subscription);
        }
        if let Some(bar) = &self.find_bar {
            let lines = Arc::clone(self.shown());
            bar.update(cx, |bar, cx| bar.set_lines(lines, cx));
            let query_focus = bar.read(cx).focus_handle(cx);
            window.focus(&query_focus, cx);
        }
        cx.notify();
    }

    /// The find bar's events: a new current match scrolls into view, and Escape closes
    /// the bar and hands focus back to the body.
    fn on_find_event(
        &mut self,
        bar: &Entity<FindBar>,
        event: &FindEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FindEvent::Changed => {
                if let Some(line) = bar.read(cx).current_line() {
                    self.list_scroll_handle
                        .scroll_to_item(line, ScrollStrategy::Nearest);
                }
                cx.notify();
            }
            FindEvent::Dismissed => {
                self.find_bar = None;
                self.find_subscription = None;
                window.focus(&self.focus_handle, cx);
                cx.notify();
            }
        }
    }

    /// Tell the find bar about new shown lines. The match list recomputes there — this
    /// only moves the data.
    fn refresh_find(&mut self, cx: &mut Context<Self>) {
        if let Some(bar) = &self.find_bar {
            let lines = Arc::clone(self.shown());
            bar.update(cx, |bar, cx| bar.set_lines(lines, cx));
        }
    }

    /// The find bar's current matches (empty while it is closed) — what the visible rows
    /// highlight. The list itself is computed in [`FindBar`], never here.
    fn find_matches(&self, cx: &App) -> Arc<Vec<FindMatch>> {
        match &self.find_bar {
            Some(bar) => bar.read(cx).matches(),
            None => Arc::new(Vec::new()),
        }
    }

    /// Override the detected formatter by hand — [`None`] returns to the `Content-Type`
    /// choice. The Pretty lines describe the old formatter, so they reformat — and the
    /// find bar re-syncs to the lines on screen meanwhile, the same way [`Self::set_mode`]
    /// keeps it in step.
    pub fn set_formatter_override(&mut self, formatter: Option<Formatter>, cx: &mut Context<Self>) {
        self.formatter_override = formatter;
        self.refresh_format(cx);
        self.refresh_find(cx);
        cx.notify();
    }

    /// The Raw mode lines: the decoded text exactly, one entry per line.
    pub fn lines(&self) -> &[SharedString] {
        &self.lines
    }

    /// The mode the pane draws in — what the large-body fallback switches to Raw.
    #[cfg(test)]
    pub(crate) fn mode(&self) -> BodyMode {
        self.mode
    }

    /// The large-body fallback in effect (REQ-RESP-06): [`Some`] while the body on
    /// screen is over the pretty limit — the banner's state and its reveal target.
    #[cfg(test)]
    pub(crate) fn large_body(&self) -> Option<&LargeBodyNotice> {
        self.large_body.as_ref()
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

    /// Pretty mode's syntax spans, one list per formatted line.
    #[cfg(test)]
    pub(crate) fn syntax_spans(&self) -> &[LineSpans] {
        &self.syntax_spans
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
        self.syntax_spans = Arc::new(Vec::new());
        self.format_error = None;
        if self.mode != BodyMode::Pretty || self.text.is_empty() {
            return;
        }
        let text = Arc::clone(&self.text);
        let formatter = self.effective_formatter();
        self.format_task = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    let formatted = formatter
                        .format(text.as_bytes())
                        .map_err(|error| format_error_note(formatter, error))?;
                    let lines = split_lines(&formatted);
                    let spans = if formatter == Formatter::Json {
                        lines.iter().map(|line| json_line_spans(line)).collect()
                    } else {
                        Vec::new()
                    };
                    Ok((lines, spans))
                })
                .await;
            if let Err(_released) = this.update(cx, |view, cx| {
                match outcome {
                    Ok((lines, spans)) => {
                        view.pretty_lines = Arc::new(lines);
                        view.syntax_spans = Arc::new(spans);
                    }
                    Err(message) => view.format_error = Some(message),
                }
                view.refresh_find(cx);
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
            .py(DynamicSpacing::Base06.rems(cx))
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
        let row_spans = if Arc::ptr_eq(&lines, &self.pretty_lines) {
            Arc::clone(&self.syntax_spans)
        } else {
            Arc::new(Vec::new())
        };
        let row_matches = self.find_matches(cx);
        let row_selection = self.active_selection().cloned();
        let row_view = cx.entity().downgrade();
        let row_base = base.clone();
        let list_base = base.clone();
        let render_base = base.clone();
        let list = VirtualList::new(
            child_id(&list_base, "lines"),
            lines.len(),
            move |index| child_id(&row_base, format!("line-{index}")),
            move |index, _window, cx| {
                let spans = row_spans.get(index).map(Vec::as_slice).unwrap_or_default();
                let selected = row_selection
                    .as_ref()
                    .and_then(|selection| selection.range_on(index));
                render_line_with_matches(
                    &row_lines,
                    index,
                    &render_base,
                    LineDecorations {
                        spans,
                        matches: &row_matches,
                        selected,
                        view: Some(row_view.clone()),
                    },
                    cx,
                )
            },
        )
        .track_scroll(self.list_scroll_handle.clone());
        let drag_view = cx.entity().downgrade();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .child(
                // A window-wide listener for the rows' bounds: a selecting drag that
                // leaves the rows — below the last one, or out of the pane — still
                // reaches the view, which a row's own hover-bound handler never sees.
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let move_view = drag_view.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble
                                || event.pressed_button != Some(MouseButton::Left)
                            {
                                return;
                            }
                            let position = event.position;
                            if move_view
                                .update(cx, |view, cx| view.drag_beyond_rows(position, bounds, cx))
                                .is_err()
                            {
                                // The pane is gone; there is nothing left to select.
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                                return;
                            }
                            if drag_view
                                .update(cx, |view, _| view.end_selection())
                                .is_err()
                            {
                                // The pane is gone; there is no drag left to end.
                            }
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
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
            .key_context(BODY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event, _window, _cx| this.end_selection()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event, _window, _cx| this.end_selection()),
            )
            .child(self.render_toolbar(cx))
            .children(self.find_bar.clone())
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
            .map(|this| match self.large_body.clone() {
                Some(notice) => this.child(
                    LargeBodyBanner::new(child_id(&base, "large-body"), &notice).on_reveal(
                        cx.listener(|this, _event, _window, cx| this.reveal_large_body_file(cx)),
                    ),
                ),
                None => this,
            })
            .map(|this| match self.mode {
                BodyMode::Pretty | BodyMode::Raw => this.child(self.render_lines(&base, cx)),
                BodyMode::Hex => this.child(self.render_hex(&base, cx)),
                BodyMode::Preview => this.child(self.render_preview(&base, cx)),
            })
            .children(self.body_menu.clone().map(|(menu, position)| {
                deferred(
                    anchored()
                        .position(position)
                        .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                        .child(menu),
                )
                .with_priority(1)
            }))
    }
}

impl Focusable for BodyView {
    /// The pane itself is a focus target so Escape from the find bar has somewhere to
    /// return to ([`Self::on_find_event`]).
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// One body line: the content font, truncated with its full text in a hover tooltip
/// when — and only when — the line is actually truncated (#268). Shared with the Preview
/// text lines ([`crate::preview`]): a body pane's lines all draw the same way.
pub(crate) fn render_line(
    lines: &[SharedString],
    index: usize,
    base: &ElementId,
    cx: &mut App,
) -> AnyElement {
    render_line_with_matches(lines, index, base, LineDecorations::default(), cx)
}

/// What a body line draws over its plain text, and the pane its presses select in.
#[derive(Default)]
struct LineDecorations<'a> {
    spans: &'a [(Range<usize>, JsonToken)],
    matches: &'a [FindMatch],
    /// The bytes of the line inside the hand-made selection.
    selected: Option<Range<usize>>,
    /// The pane that owns the selection; [`None`] where lines are not selectable.
    view: Option<WeakEntity<BodyView>>,
}

/// One body line, coloured by its syntax spans, with the find matches on it painted as
/// [`HighlightStyle`] backgrounds over them (REQ-RESP-03). Only visible rows get here
/// ([`VirtualList`] asks per row), and the row's matches come out of the ordered list by
/// one binary search.
fn render_line_with_matches(
    lines: &[SharedString],
    index: usize,
    base: &ElementId,
    decorations: LineDecorations,
    cx: &App,
) -> AnyElement {
    let LineDecorations {
        spans,
        matches,
        selected,
        view,
    } = decorations;
    let Some(line) = lines.get(index).cloned() else {
        return div().into_any_element();
    };
    let highlight = HighlightStyle {
        background_color: Some(cx.theme().colors().search_match_background),
        ..HighlightStyle::default()
    };
    let first = matches.partition_point(|found| found.line < index);
    let found = matches[first..]
        .iter()
        .take_while(|found| found.line == index)
        .map(|found| (found.range.clone(), highlight))
        .collect::<Vec<_>>();
    let colours = cx.theme().colors();
    let syntax = spans
        .iter()
        .map(|(range, token)| {
            let color = match token {
                JsonToken::Key => colours.syntax_key,
                JsonToken::String => colours.syntax_string,
                JsonToken::Number => colours.syntax_number,
                JsonToken::Boolean => colours.syntax_boolean,
                JsonToken::Null => colours.syntax_null,
                JsonToken::Punctuation => colours.syntax_punctuation,
            };
            let style = HighlightStyle {
                color: Some(color),
                ..HighlightStyle::default()
            };
            (range.clone(), style)
        })
        .collect::<Vec<_>>();
    let selection = selected
        .map(|range| {
            let style = HighlightStyle {
                background_color: Some(colours.element_selection_background),
                ..HighlightStyle::default()
            };
            vec![(range, style)]
        })
        .unwrap_or_default();
    let highlights = layer_highlights(&layer_highlights(&syntax, &selection), &found);
    let line_id = child_id(base, format!("line-{index}"));
    let text = StyledText::new(line.clone()).with_highlights(highlights);
    // The layout the text fills once the list lays it out — read at hover time to see
    // what was actually drawn.
    let laid_out = text.layout().clone();
    let press_layout = laid_out.clone();
    let drag_layout = laid_out.clone();
    let full_line = line;
    let press_line = full_line.clone();
    let drag_line = full_line.clone();
    let menu_layout = laid_out.clone();
    let menu_line = full_line.clone();
    let press_view = view.clone();
    let menu_view = view.clone();
    let drag_view = view;
    div()
        .id(child_id(&line_id, "text"))
        .when_some(press_view, |row, view| {
            row.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let at = BodyPoint {
                    line: index,
                    column: column_at(&press_layout, &press_line, event.position),
                };
                let extend = event.modifiers.shift;
                let click_count = event.click_count;
                if view
                    .update(cx, |view, cx| {
                        view.begin_selection(at, extend, click_count, window, cx)
                    })
                    .is_err()
                {
                    // The pane closed under the pointer; there is nothing left to select.
                }
            })
        })
        .when_some(menu_view, |row, view| {
            row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                let at = BodyPoint {
                    line: index,
                    column: column_at(&menu_layout, &menu_line, event.position),
                };
                let position = event.position;
                // The pane tracks focus, and GPUI refocuses a tracked element on any press
                // that is not default-prevented — which would pull focus straight back
                // off the menu this press opens.
                window.prevent_default();
                if view
                    .update(cx, |view, cx| view.open_body_menu(at, position, window, cx))
                    .is_err()
                {
                    // The pane closed under the pointer; there is nothing left to copy.
                }
            })
        })
        .when_some(drag_view, |row, view| {
            row.on_mouse_move(move |event, _window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let at = BodyPoint {
                    line: index,
                    column: column_at(&drag_layout, &drag_line, event.position),
                };
                if view
                    .update(cx, |view, cx| view.extend_selection(at, cx))
                    .is_err()
                {
                    // The pane closed under the pointer; there is nothing left to select.
                }
            })
        })
        .w_full()
        .min_w_0()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base01.rems(cx))
        .font_buffer(cx)
        .text_buffer(cx)
        .text_color(cx.theme().colors().text)
        .child(div().w_full().min_w_0().truncate().child(
            InteractiveText::new(child_id(&line_id, "hover"), text).tooltip(
                move |_index, _window, cx| {
                    // A truncation tooltip only (#268): when the drawn text is the
                    // line we hold, the hover has nothing to add — a balloon of the
                    // same plain text would only cover the token-coloured line it
                    // repeats (`seg2.png`).
                    if laid_out.text() == full_line {
                        None
                    } else {
                        Some(Tooltip::text(full_line.clone()).into_any_view(cx))
                    }
                },
            ),
        ))
        .into_any_element()
}

/// The byte column of `line` under the pointer's x. The row's text is one visual line,
/// so the pointer's y is pinned to it: a drag that wanders above or below the text still
/// selects by x.
fn column_at(layout: &TextLayout, line: &str, position: Point<Pixels>) -> usize {
    let at = point(position.x, layout.bounds().center().y);
    let index = match layout.index_for_position(at) {
        Ok(index) | Err(index) => index,
    };
    clamp_column(line, index)
}

/// How often a drag held past the rows' edge scrolls them another step.
const AUTOSCROLL_INTERVAL: Duration = Duration::from_millis(30);
/// The most rows one autoscroll step moves, however far past the edge the pointer is.
const AUTOSCROLL_MAX_ROWS: f32 = 8.;

/// What a selecting press selects by: characters, or the word or line it landed on (as
/// the range a drag must keep), or everything.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SelectUnit {
    Character,
    Word((BodyPoint, BodyPoint)),
    Line((BodyPoint, BodyPoint)),
    All,
}

/// The word, run of punctuation or gap under `at`, as the text inputs see one.
fn word_at(lines: &[SharedString], at: BodyPoint) -> (BodyPoint, BodyPoint) {
    let line = lines.get(at.line).map_or("", |line| line.as_ref());
    let range = word_range_at(line, at.column);
    (
        BodyPoint {
            line: at.line,
            column: range.start,
        },
        BodyPoint {
            line: at.line,
            column: range.end,
        },
    )
}

/// The line under `at` with its line break: through the start of the next line, or to
/// the end of the last one.
fn line_at(lines: &[SharedString], at: BodyPoint) -> (BodyPoint, BodyPoint) {
    let start = BodyPoint {
        line: at.line,
        column: 0,
    };
    if at.line + 1 < lines.len() {
        (
            start,
            BodyPoint {
                line: at.line + 1,
                column: 0,
            },
        )
    } else {
        (start, end_of_line(lines, at.line))
    }
}

fn end_of_line(lines: &[SharedString], line: usize) -> BodyPoint {
    BodyPoint {
        line,
        column: lines.get(line).map_or(0, |text| text.len()),
    }
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

#[cfg(test)]
#[path = "body_tooltip_tests.rs"]
mod tooltip_tests;

#[cfg(test)]
#[path = "find_format_tests.rs"]
mod format_tests;

#[cfg(test)]
#[path = "json_syntax_view_tests.rs"]
mod json_syntax_view_tests;

#[cfg(test)]
#[path = "body_mode_bar_tests.rs"]
mod body_mode_bar_tests;

#[cfg(test)]
#[path = "body_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "body_menu_tests.rs"]
mod menu_tests;

#[cfg(test)]
#[path = "body_multi_click_tests.rs"]
mod multi_click_tests;
