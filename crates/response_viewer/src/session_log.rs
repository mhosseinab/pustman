//! The Message log (REQ-LOG-01/02, REQ-WS-04): one streaming session's events as a
//! virtualized list. Every row shows the direction (↑ sent, ↓ received, • system), the
//! time — absolute or relative to the session's first message, behind a toggle — the
//! type, the size and a one-line preview. Enter or a click expands a row into its full
//! payload (pretty JSON, text, or hexdump rows for binary) and its protocol fields.
//!
//! The log reads a [`SessionStore`] and never copies it: the rows are the store's
//! [`SessionStore::filtered`] events, so the filter decides what is listed. The store's
//! payloads are already redacted, and nothing here logs a payload.
//!
//! `Decision:` an expanded row's detail is lines of the same [`VirtualList`] as the row
//! summaries (the `timeline` redirect-list shape), so every line stays one row tall and a
//! hexdump of a megabyte stays virtualized. A line's element id comes from its event's
//! sequence number — the event's place in the store, which never changes while the
//! session lives — and the line's place in that event's detail, never from the row index.
//! A cleared store starts its numbering again, so the log forgets every expansion then.
//!
//! `Decision:` the detail is computed when the row is expanded, on a background task
//! (pretty JSON, text lines, hex rows), and cached until it collapses; render only lays
//! the cached lines out. A spilled payload shows its in-memory prefix (as text — a cut
//! JSON document cannot be pretty-printed — or hex) with a note of how much it is, plus
//! "Open full payload": that reads the spill file off the foreground and formats the
//! whole payload; a failed read is shown on that line with the action to retry.
//!
//! `Decision:` memory has caps (S51c review). "Open full payload" loads at most
//! [`FULL_PAYLOAD_CAP`] (64 MB) into one buffer; a bigger payload says it is too large
//! to open in the log and points at the export, with no button. JSON over
//! [`PRETTY_CAP`] (8 MB) is shown as text, not pretty-printed. Every detail line is cut
//! at [`LINE_CHARS`] (4,000) characters with a note of how many were left out, when the
//! detail is built, so no huge line is shaped on the foreground each frame.
//!
//! `Decision:` the keyboard path is the list itself, one tab stop: Up/Down (and Home/End)
//! move the selected row, Enter or Space expands or collapses it (`timeline`'s
//! `handle_key_down` precedent — the keymaps have no Message log context yet). A click
//! selects, focuses and toggles the row.
//!
//! `Decision:` relative time counts from the session's first message: lifecycle events
//! carry no timestamp of their own (`api_model::SessionEvent`), so they show none.
//! Absolute time is the UTC time of day, the History panel's clock.
//!
//! `Decision:` the log follows every new row to the bottom unless the toolbar's Pause
//! holds it (REQ-LOG-03, [`crate::log_toolbar`]). The toolbar sits in the header, after
//! the time toggle; this file only mounts it and registers its actions.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use api_model::{
    CloseReason, Message, Payload, ProtocolFields, SessionEvent, SessionMessageKind,
    WebSocketOpcode,
};
use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FocusHandle, Focusable, KeyDownEvent,
    Role, ScrollStrategy, Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
};
use time::OffsetDateTime;
use ui::format::{format_duration, format_size};
use ui::prelude::*;

use crate::child_id;
use crate::hex_view::HexRows;
use crate::log_toolbar::ToolbarState;
use crate::pretty::Formatter;
use crate::session_store::SessionStore;

/// Every line of the log is this tall: the redesign pack's `compact-row`.
const ROW_HEIGHT_PX: f32 = 32.;
/// The widest the fixed columns get, so the preview starts at the same place on every row.
const TIME_COLUMN_PX: f32 = 96.;
const KIND_COLUMN_PX: f32 = 72.;
const SIZE_COLUMN_PX: f32 = 72.;
const FIELD_LABEL_PX: f32 = 120.;
/// The most characters a one-line preview takes from a payload.
const PREVIEW_CHARS: usize = 200;
/// The most characters a tooltip shows: a one-megabyte line belongs in the row, not in
/// a balloon.
const TOOLTIP_CHARS: usize = 2000;
/// The bytes a binary preview shows as hex.
const BINARY_PREVIEW_BYTES: usize = 16;

/// The label of the action that loads a spilled payload whole.
const OPEN_FULL_PAYLOAD: &str = "Open full payload";

/// The largest spilled payload "Open full payload" loads into memory. Its lines and
/// their strings take a few times its size, so a bigger one is left to the export.
pub(crate) const FULL_PAYLOAD_CAP: u64 = 64 * 1024 * 1024;
/// The largest payload that is pretty-printed: the formatted copy and its lines are
/// several times the payload, so a bigger JSON document is shown as the text it is.
const PRETTY_CAP: usize = 8 * 1024 * 1024;
/// The most characters one detail line keeps; the rest is named, not shaped every frame.
const LINE_CHARS: usize = 4_000;

/// How the time column reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeMode {
    /// The UTC time of day, to the millisecond.
    Absolute,
    /// The time since the session's first message.
    Relative,
}

/// What one summary row shows. Built for the rows on screen only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSummary {
    /// `↑`, `↓` or `•`.
    pub glyph: SharedString,
    /// The glyph's accessible name: "Sent", "Received" or "System".
    pub direction: SharedString,
    pub time: SharedString,
    pub kind: SharedString,
    pub size: SharedString,
    /// One line, at most [`PREVIEW_CHARS`] characters.
    pub preview: SharedString,
}

/// A summary row or a line of an expanded row's detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineRef {
    Summary { seq: usize },
    Detail { seq: usize, line: usize },
}

/// One expanded event in the list: where its summary line is and how many detail lines
/// follow it.
#[derive(Clone, Copy)]
struct Segment {
    seq: usize,
    summary_line: usize,
    len: usize,
}

/// The listed events' sequence numbers, as the list's `'static` closures hold them.
enum Rows {
    /// No filter is hiding anything: row `n` is event `n`, so nothing is copied.
    Identity(usize),
    /// A filter is on: one copy of the store's matching rows per frame. The row-key
    /// closure cannot borrow the store, and a copy kept per push would be quadratic.
    Filtered(Arc<[usize]>),
}

impl Rows {
    fn new(filtered: &[usize], event_count: usize) -> Self {
        // The filtered rows are strictly increasing event indices, so as many rows as
        // events means every event, in order.
        if filtered.len() == event_count {
            Self::Identity(event_count)
        } else {
            Self::Filtered(Arc::from(filtered))
        }
    }

    fn get(&self, position: usize) -> Option<usize> {
        match self {
            Self::Identity(len) => (position < *len).then_some(position),
            Self::Filtered(rows) => rows.get(position).copied(),
        }
    }
}

/// Where each expanded, listed event's summary line is and how many detail lines follow
/// it. Built from the expanded events only (a handful), never from every row.
fn segments(rows: &[usize], expanded: &BTreeMap<usize, Detail>) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut shift = 0;
    for (&seq, detail) in expanded {
        let Ok(position) = rows.binary_search(&seq) else {
            continue;
        };
        let len = detail.line_count();
        segments.push(Segment {
            seq,
            summary_line: position + shift,
            len,
        });
        shift += len;
    }
    segments
}

/// How many list lines the rows and their expansions take.
fn line_count(row_count: usize, segments: &[Segment]) -> usize {
    row_count + segments.iter().map(|segment| segment.len).sum::<usize>()
}

/// The list line of the event's summary row, when the event is listed.
fn summary_line(rows: &[usize], segments: &[Segment], seq: usize) -> Option<usize> {
    let position = rows.binary_search(&seq).ok()?;
    let shift: usize = segments
        .iter()
        .take_while(|segment| segment.seq < seq)
        .map(|segment| segment.len)
        .sum();
    Some(position + shift)
}

/// Which list line is which, for one frame.
struct Layout {
    rows: Rows,
    segments: Vec<Segment>,
}

impl Layout {
    fn line(&self, index: usize) -> Option<LineRef> {
        let mut shift = 0;
        for segment in &self.segments {
            if index <= segment.summary_line {
                break;
            }
            if index <= segment.summary_line + segment.len {
                return Some(LineRef::Detail {
                    seq: segment.seq,
                    line: index - segment.summary_line - 1,
                });
            }
            shift += segment.len;
        }
        let seq = self.rows.get(index.checked_sub(shift)?)?;
        Some(LineRef::Summary { seq })
    }
}

/// The payload part of an expanded row.
enum DetailBody {
    /// Nothing to show: a lifecycle event.
    None,
    /// Being formatted on a background task.
    Formatting,
    Lines(Arc<Vec<SharedString>>),
    Hex(Arc<HexRows>),
}

/// A spilled payload the user has not opened whole yet.
struct SpillSource {
    path: PathBuf,
    prefix: Arc<Vec<u8>>,
    /// The whole payload's size, at most [`FULL_PAYLOAD_CAP`].
    len: u64,
    kind: SessionMessageKind,
    loading: bool,
    error: Option<SharedString>,
}

/// One expanded event's detail, cached from expansion until it collapses.
struct Detail {
    fields: Vec<(SharedString, SharedString)>,
    note: Option<SharedString>,
    body: DetailBody,
    spill: Option<SpillSource>,
    /// The formatting or the full-payload read; dropping it with the detail cancels it.
    _task: Option<Task<()>>,
}

impl Detail {
    fn line_count(&self) -> usize {
        let body = match &self.body {
            DetailBody::None => 0,
            DetailBody::Formatting => 1,
            DetailBody::Lines(lines) => lines.len(),
            DetailBody::Hex(rows) => rows.row_count(),
        };
        self.fields.len()
            + usize::from(self.note.is_some())
            + body
            + usize::from(self.spill.is_some())
    }

    fn line(&self, line: usize) -> Option<DetailLine<'_>> {
        if let Some((label, value)) = self.fields.get(line) {
            return Some(DetailLine::Field { label, value });
        }
        let mut line = line - self.fields.len();
        if let Some(note) = &self.note {
            if line == 0 {
                return Some(DetailLine::Note(note));
            }
            line -= 1;
        }
        let body_len = match &self.body {
            DetailBody::None => 0,
            DetailBody::Formatting => {
                if line == 0 {
                    return Some(DetailLine::Formatting);
                }
                1
            }
            DetailBody::Lines(lines) => match lines.get(line) {
                Some(text) => return Some(DetailLine::Text(text)),
                None => lines.len(),
            },
            DetailBody::Hex(rows) => match rows.row(line) {
                Some(row) => {
                    return Some(DetailLine::Hex(
                        format!("{}  {}  {}", row.offset, row.hex, row.ascii).into(),
                    ));
                }
                None => rows.row_count(),
            },
        };
        match &self.spill {
            Some(spill) if line == body_len => Some(DetailLine::OpenFull {
                loading: spill.loading,
                error: spill.error.as_ref(),
            }),
            _ => None,
        }
    }
}

/// One line of an expanded row's detail.
enum DetailLine<'a> {
    Field {
        label: &'a SharedString,
        value: &'a SharedString,
    },
    Note(&'a SharedString),
    Formatting,
    Text(&'a SharedString),
    Hex(SharedString),
    OpenFull {
        loading: bool,
        error: Option<&'a SharedString>,
    },
}

impl DetailLine<'_> {
    /// The line as text, the way it reads on screen.
    fn text(&self) -> String {
        match self {
            Self::Field { label, value } => format!("{label}: {value}"),
            Self::Note(note) => note.to_string(),
            Self::Formatting => "Formatting…".to_owned(),
            Self::Text(text) => text.to_string(),
            Self::Hex(row) => row.to_string(),
            Self::OpenFull { loading: true, .. } => "Opening full payload…".to_owned(),
            Self::OpenFull {
                error: Some(error), ..
            } => error.to_string(),
            Self::OpenFull { .. } => OPEN_FULL_PAYLOAD.to_owned(),
        }
    }
}

/// The Message log: a [`SessionStore`] as a virtualized, expandable list.
pub struct SessionLog {
    pub(crate) store: Entity<SessionStore>,
    pub(crate) base: ElementId,
    focus_handle: FocusHandle,
    scroll: UniformListScrollHandle,
    time_mode: TimeMode,
    expanded: BTreeMap<usize, Detail>,
    pub(crate) selected: Option<usize>,
    /// The search field, filters, pause and pending export/copy ([`crate::log_toolbar`]).
    pub(crate) toolbar: ToolbarState,
    /// When the session's first message happened, the zero of relative time.
    origin: Option<u64>,
    /// How far the store's events were searched for [`Self::origin`].
    origin_scanned: usize,
    seen_len: usize,
    seen_rows: usize,
    _subscriptions: Vec<Subscription>,
}

impl SessionLog {
    pub fn new(store: Entity<SessionStore>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&store, |log, store, cx| log.on_store_changed(store, cx));
        let mut log = Self {
            base: ElementId::from(("session-log", cx.entity_id())),
            focus_handle: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            time_mode: TimeMode::Absolute,
            expanded: BTreeMap::new(),
            selected: None,
            toolbar: ToolbarState::default(),
            origin: None,
            origin_scanned: 0,
            seen_len: 0,
            seen_rows: 0,
            _subscriptions: vec![subscription],
            store: store.clone(),
        };
        let (len, rows) = {
            let store = store.read(cx);
            (store.len(), store.filtered().len())
        };
        log.scan_origin(cx);
        log.seen_len = len;
        log.seen_rows = rows;
        log
    }

    /// The store the log shows.
    pub fn store(&self) -> &Entity<SessionStore> {
        &self.store
    }

    pub fn time_mode(&self) -> TimeMode {
        self.time_mode
    }

    pub fn set_time_mode(&mut self, mode: TimeMode, cx: &mut Context<Self>) {
        self.time_mode = mode;
        cx.notify();
    }

    /// The element id (and test selector) of the summary row of the event numbered `seq`.
    pub fn row_element_id(&self, seq: usize) -> ElementId {
        child_id(&self.base, format!("event-{seq}"))
    }

    /// The element id of line `line` of the event's expanded detail.
    pub fn detail_line_element_id(&self, seq: usize, line: usize) -> ElementId {
        child_id(&self.base, format!("event-{seq}-line-{line}"))
    }

    pub fn is_expanded(&self, seq: usize) -> bool {
        self.expanded.contains_key(&seq)
    }

    /// The summary of the event numbered `seq`, as its row shows it.
    pub fn summary(&self, seq: usize, cx: &App) -> Option<RowSummary> {
        let event = self.store.read(cx).events().get(seq)?;
        Some(summarize(event, self.time_mode, self.origin))
    }

    /// Every line of the event's expanded detail as text, or `None` while it is collapsed.
    pub fn detail_text(&self, seq: usize) -> Option<Vec<String>> {
        let detail = self.expanded.get(&seq)?;
        Some(
            (0..detail.line_count())
                .filter_map(|line| detail.line(line).map(|line| line.text()))
                .collect(),
        )
    }

    /// Expand the event's row, or collapse it when it is expanded.
    pub fn toggle(&mut self, seq: usize, cx: &mut Context<Self>) {
        if self.expanded.remove(&seq).is_none() {
            self.expand(seq, cx);
        }
        cx.notify();
    }

    fn expand(&mut self, seq: usize, cx: &mut Context<Self>) {
        let Some(event) = self.store.read(cx).events().get(seq) else {
            return;
        };
        let fields = detail_fields(event);
        let Some(message) = message_of(event) else {
            self.expanded.insert(
                seq,
                Detail {
                    fields,
                    note: None,
                    body: DetailBody::None,
                    spill: None,
                    _task: None,
                },
            );
            return;
        };
        let kind = message.kind;
        let (bytes, note, spill) = match &message.payload {
            Payload::Inline(bytes) => (Arc::new(bytes.clone()), None, None),
            Payload::Spilled { path, len, prefix } => {
                let prefix = Arc::new(prefix.clone());
                let shown = format_size(u64::try_from(prefix.len()).unwrap_or(u64::MAX));
                let (note, spill) = if *len > FULL_PAYLOAD_CAP {
                    let note = format!(
                        "Showing the first {shown} of {}. Too large to open in the log; \
                         export the session to read it.",
                        format_size(*len)
                    );
                    (note, None)
                } else {
                    let note = format!("Showing the first {shown} of {}.", format_size(*len));
                    let spill = SpillSource {
                        path: path.clone(),
                        prefix: Arc::clone(&prefix),
                        len: *len,
                        kind,
                        loading: false,
                        error: None,
                    };
                    (note, Some(spill))
                };
                (prefix, Some(SharedString::from(note)), spill)
            }
        };
        let whole = matches!(message.payload, Payload::Inline(_));
        let task = self.format_in_background(seq, kind, bytes, whole, cx);
        self.expanded.insert(
            seq,
            Detail {
                fields,
                note,
                body: DetailBody::Formatting,
                spill,
                _task: Some(task),
            },
        );
    }

    /// Format `bytes` for the event's detail off the foreground. `whole` is false for a
    /// spilled payload's prefix, which is never pretty-printed: a cut JSON document does
    /// not parse.
    fn format_in_background(
        &self,
        seq: usize,
        kind: SessionMessageKind,
        bytes: Arc<Vec<u8>>,
        whole: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let (body, note) = cx
                .background_spawn(async move { format_payload(kind, bytes, whole) })
                .await;
            if let Err(_released) = this.update(cx, |log, cx| {
                if let Some(detail) = log.expanded.get_mut(&seq) {
                    detail.body = body;
                    if note.is_some() {
                        detail.note = note;
                    }
                    cx.notify();
                }
            }) {
                // Handled: the log went away with its window; nobody is left to show
                // the payload to.
            }
        })
    }

    /// Read a spilled payload whole, off the foreground, and show it formatted. A failed
    /// read is shown on the "Open full payload" line, which stays to retry.
    pub fn open_full_payload(&mut self, seq: usize, cx: &mut Context<Self>) {
        let Some(detail) = self.expanded.get_mut(&seq) else {
            return;
        };
        let Some(spill) = detail.spill.as_mut() else {
            return;
        };
        if spill.loading {
            return;
        }
        spill.loading = true;
        spill.error = None;
        let path = spill.path.clone();
        let prefix = Arc::clone(&spill.prefix);
        let len = spill.len;
        let kind = spill.kind;
        detail._task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    let bytes = read_spilled(&path, &prefix, len)?;
                    Ok::<_, std::io::Error>(format_payload(kind, Arc::new(bytes), true))
                })
                .await;
            if let Err(_released) = this.update(cx, |log, cx| {
                let Some(detail) = log.expanded.get_mut(&seq) else {
                    return;
                };
                match loaded {
                    Ok((body, note)) => {
                        detail.body = body;
                        detail.note = note;
                        detail.spill = None;
                    }
                    Err(error) => {
                        if let Some(spill) = detail.spill.as_mut() {
                            spill.loading = false;
                            spill.error = Some(
                                format!(
                                    "Could not read the full payload: {error}. Try opening it again."
                                )
                                .into(),
                            );
                        }
                    }
                }
                cx.notify();
            }) {
                // Handled: the log went away with its window; nobody is left to show
                // the payload to.
            }
        }));
        cx.notify();
    }

    fn on_store_changed(&mut self, store: Entity<SessionStore>, cx: &mut Context<Self>) {
        let (len, rows) = {
            let store = store.read(cx);
            (store.len(), store.filtered().len())
        };
        if len < self.seen_len {
            // Cleared: the numbering starts again, so no old expansion may stick to a
            // new event.
            self.expanded.clear();
            self.selected = None;
            self.origin = None;
            self.origin_scanned = 0;
        }
        self.scan_origin(cx);
        if len > self.seen_len && rows > self.seen_rows && !self.toolbar.paused() {
            self.scroll_to_end(cx);
        }
        self.seen_len = len;
        self.seen_rows = rows;
        cx.notify();
    }

    /// Scroll the list to its last line.
    pub(crate) fn scroll_to_end(&self, cx: &App) {
        let filtered = self.store.read(cx).filtered();
        let lines = line_count(filtered.len(), &segments(filtered, &self.expanded));
        if let Some(last) = lines.checked_sub(1) {
            self.scroll.scroll_to_item(last, ScrollStrategy::Bottom);
        }
    }

    /// Find the session's first message, looking only at events not searched yet.
    fn scan_origin(&mut self, cx: &App) {
        if self.origin.is_some() {
            return;
        }
        let events = self.store.read(cx).events();
        self.origin = events
            .get(self.origin_scanned..)
            .into_iter()
            .flatten()
            .find_map(|event| message_of(event).map(|message| message.at));
        self.origin_scanned = events.len();
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keys bubbling up from a control inside the log (the time toggle, "Open full
        // payload") belong to that control: Enter there must not toggle the row.
        if event.keystroke.modifiers.modified() || !self.focus_handle.is_focused(window) {
            return;
        }
        let rows = self.store.read(cx).filtered();
        let position = self.selected.and_then(|seq| rows.binary_search(&seq).ok());
        let target = match (event.keystroke.key.as_str(), position) {
            ("down", Some(position)) => rows.get(position + 1).or(rows.get(position)),
            ("down", None) | ("home", _) => rows.first(),
            ("up", Some(position)) => rows.get(position.saturating_sub(1)),
            ("up", None) | ("end", _) => rows.last(),
            ("enter" | "space", _) => {
                if let Some(seq) = self.selected {
                    cx.stop_propagation();
                    self.toggle(seq, cx);
                }
                return;
            }
            _ => return,
        };
        let Some(&seq) = target else {
            cx.stop_propagation();
            return;
        };
        let line = summary_line(rows, &segments(rows, &self.expanded), seq);
        cx.stop_propagation();
        self.selected = Some(seq);
        if let Some(line) = line {
            self.scroll.scroll_to_item(line, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn render_header(&self, cx: &Context<Self>) -> AnyElement {
        let (label, next) = match self.time_mode {
            TimeMode::Absolute => ("Absolute time", TimeMode::Relative),
            TimeMode::Relative => ("Relative time", TimeMode::Absolute),
        };
        let view = cx.entity().downgrade();
        h_flex()
            .w_full()
            .items_start()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                Button::new(child_id(&self.base, "time-mode"), label)
                    .size(ButtonSize::Compact)
                    .start_icon(Icon::new(IconName::Clock).size(IconSize::Small))
                    .tooltip(Tooltip::text(match next {
                        TimeMode::Relative => "Show the time since the first message",
                        TimeMode::Absolute => "Show the time of day (UTC)",
                    }))
                    .on_click(move |_event, _window, cx| {
                        if view
                            .update(cx, |log, cx| log.set_time_mode(next, cx))
                            .is_err()
                        {
                            // Handled: the log went away with its window.
                        }
                    }),
            )
            .child(div().flex_1().min_w_0().child(self.render_toolbar(cx)))
            .into_any_element()
    }
}

impl Focusable for SessionLog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SessionLog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_toolbar(window, cx);
        let header = self.render_header(cx);
        let root = Self::register_toolbar_actions(v_flex().id(child_id(&self.base, "root")), cx);
        let store = self.store.read(cx);
        let focused = self.focus_handle.contains_focused(window, cx);
        let root = root
            .size_full()
            .min_w_0()
            .min_h_0()
            .track_focus(&self.focus_handle)
            .key_context("SessionLog")
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(header);
        if store.filtered().is_empty() {
            let empty = if store.is_empty() {
                EmptyState::new(
                    child_id(&self.base, "empty"),
                    IconName::Terminal,
                    "No messages yet",
                )
                .description("Messages sent and received in this session appear here.")
            } else {
                EmptyState::new(
                    child_id(&self.base, "empty"),
                    IconName::Filter,
                    "No matching messages",
                )
                .description("Nothing in this session matches the filter.")
            };
            return root.child(div().flex_1().min_h_0().child(empty));
        }

        let segments = segments(store.filtered(), &self.expanded);
        let lines = line_count(store.filtered().len(), &segments);
        let layout = Rc::new(Layout {
            rows: Rows::new(store.filtered(), store.len()),
            segments,
        });
        let context = Rc::new(LineContext {
            view: cx.entity().downgrade(),
            focus_handle: self.focus_handle.clone(),
            base: self.base.clone(),
            focused,
        });
        let list = VirtualList::new(
            child_id(&self.base, "rows"),
            lines,
            {
                let layout = Rc::clone(&layout);
                let base = self.base.clone();
                move |index| line_id(&base, layout.line(index))
            },
            {
                let view = cx.entity();
                move |index, _window, cx| {
                    let cx: &App = cx;
                    let log = view.read(cx);
                    match layout.line(index) {
                        Some(LineRef::Summary { seq }) => log.render_summary(seq, &context, cx),
                        Some(LineRef::Detail { seq, line }) => {
                            log.render_detail_line(seq, line, &context, cx)
                        }
                        None => div().into_any_element(),
                    }
                }
            },
        )
        .track_scroll(self.scroll.clone());
        root.child(
            div()
                .id(child_id(&self.base, "list"))
                .role(Role::Log)
                .aria_label(SharedString::new_static("Message log"))
                .flex_1()
                .min_h_0()
                // The list is one tab stop: its focus ring is always drawn (transparent)
                // so focusing it never moves the rows (the `history_list` shape).
                .border_1()
                .border_color(cx.theme().colors().border_transparent)
                .when(focused, |list| {
                    list.border_color(cx.theme().colors().border_focused)
                })
                .child(list),
        )
    }
}

/// What every rendered line needs from the view.
struct LineContext {
    view: WeakEntity<SessionLog>,
    focus_handle: FocusHandle,
    base: ElementId,
    focused: bool,
}

impl SessionLog {
    fn render_summary(&self, seq: usize, context: &LineContext, cx: &App) -> AnyElement {
        let Some(event) = self.store.read(cx).events().get(seq) else {
            return div().into_any_element();
        };
        let summary = summarize(event, self.time_mode, self.origin);
        let expanded = self.expanded.contains_key(&seq);
        let selected = self.selected == Some(seq);
        let label = format!(
            "{} {}, {}, {}{}",
            summary.direction,
            summary.kind,
            summary.size,
            summary.time,
            if expanded { ", expanded" } else { "" }
        );
        let view = context.view.clone();
        let focus_handle = context.focus_handle.clone();
        let preview_tooltip = Tooltip::text(tooltip_text(&summary.preview));
        let colors = cx.theme().colors();
        h_flex()
            .id(child_id(&context.base, format!("event-{seq}-summary")))
            .role(Role::Button)
            .aria_label(SharedString::from(label))
            .aria_expanded(expanded)
            .aria_selected(selected)
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .border_1()
            .border_color(colors.border_transparent)
            .cursor_pointer()
            .hover(|row| row.bg(colors.ghost_element_hover))
            .when(selected, |row| row.bg(colors.element_selected))
            .when(selected && context.focused, |row| {
                row.border_color(colors.border_focused)
            })
            .on_click(move |_event: &ClickEvent, window, cx| {
                window.focus(&focus_handle, cx);
                if view
                    .update(cx, |log, cx| {
                        log.selected = Some(seq);
                        log.toggle(seq, cx);
                    })
                    .is_err()
                {
                    // Handled: the log went away with its window.
                }
            })
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::Small)
                .color(Color::Muted),
            )
            .child(
                div()
                    .flex_none()
                    .w(DynamicSpacing::Base16.rems(cx))
                    .child(Label::new(summary.glyph.clone()).color(direction_color(event))),
            )
            .child(fixed_column(summary.time.clone(), TIME_COLUMN_PX))
            .child(fixed_column(summary.kind.clone(), KIND_COLUMN_PX))
            .child(fixed_column(summary.size.clone(), SIZE_COLUMN_PX))
            .child(
                div()
                    .id(child_id(&context.base, format!("event-{seq}-preview")))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .tooltip(preview_tooltip.builder())
                    .child(
                        Label::new(summary.preview)
                            .size(LabelSize::Small)
                            .truncate(),
                    ),
            )
            .into_any_element()
    }

    fn render_detail_line(
        &self,
        seq: usize,
        line: usize,
        context: &LineContext,
        cx: &App,
    ) -> AnyElement {
        let Some(detail_line) = self.expanded.get(&seq).and_then(|detail| detail.line(line)) else {
            return div().into_any_element();
        };
        let id = child_id(&context.base, format!("event-{seq}-line-{line}-content"));
        let row = h_flex()
            .id(id.clone())
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .pl(DynamicSpacing::Base32.rems(cx))
            .pr(DynamicSpacing::Base04.rems(cx))
            .bg(cx.theme().colors().element_background);
        let element = match detail_line {
            DetailLine::Field { label, value } => row
                .child(fixed_column(label.clone(), FIELD_LABEL_PX))
                .child(truncated_text(
                    child_id(&id, "value"),
                    value.clone(),
                    false,
                    cx,
                )),
            DetailLine::Note(note) => row.child(
                Label::new(note.clone())
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .truncate(),
            ),
            DetailLine::Formatting => row.child(
                Label::new(SharedString::new_static("Formatting…"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            ),
            DetailLine::Text(text) => row.child(truncated_text(
                child_id(&id, "text"),
                text.clone(),
                true,
                cx,
            )),
            DetailLine::Hex(text) => row.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .font_buffer(cx)
                    .text_buffer(cx)
                    .text_color(cx.theme().colors().text)
                    .child(text),
            ),
            DetailLine::OpenFull { loading, error } => {
                let view = context.view.clone();
                let label = if error.is_some() {
                    "Try again"
                } else {
                    OPEN_FULL_PAYLOAD
                };
                row.when_some(error.cloned(), |row, error| {
                    row.child(
                        div().flex_1().min_w_0().child(
                            Label::new(error)
                                .size(LabelSize::Small)
                                .color(Color::Error)
                                .truncate(),
                        ),
                    )
                })
                .child(
                    Button::new(child_id(&id, "open-full"), label)
                        .size(ButtonSize::Compact)
                        .start_icon(Icon::new(IconName::File).size(IconSize::Small))
                        .loading(loading)
                        .tooltip(Tooltip::text(
                            "Read the whole payload from disk and show it here",
                        ))
                        .on_click(move |_event, _window, cx| {
                            if view
                                .update(cx, |log, cx| log.open_full_payload(seq, cx))
                                .is_err()
                            {
                                // Handled: the log went away with its window.
                            }
                        }),
                )
            }
        };
        element.into_any_element()
    }
}

/// A list line's stable id: its event's sequence number, and its place in that event's
/// detail for a detail line.
fn line_id(base: &ElementId, line: Option<LineRef>) -> ElementId {
    match line {
        Some(LineRef::Summary { seq }) => child_id(base, format!("event-{seq}")),
        Some(LineRef::Detail { seq, line }) => child_id(base, format!("event-{seq}-line-{line}")),
        None => child_id(base, "missing"),
    }
}

fn fixed_column(text: SharedString, width_px: f32) -> impl IntoElement {
    div()
        .flex_none()
        .w(px_to_rems(width_px))
        .min_w_0()
        .overflow_hidden()
        .child(
            Label::new(text)
                .size(LabelSize::Small)
                .color(Color::Muted)
                .truncate(),
        )
}

/// A value that may not fit its line: truncated, with the full value in a tooltip.
fn truncated_text(id: ElementId, text: SharedString, monospace: bool, cx: &App) -> AnyElement {
    let tooltip = Tooltip::text(tooltip_text(&text));
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .when(monospace, |this| this.font_buffer(cx).text_buffer(cx))
        .text_color(cx.theme().colors().text)
        .tooltip(tooltip.builder())
        .child(Label::new(text).size(LabelSize::Small).truncate())
        .into_any_element()
}

/// A tooltip's text: the value, cut at [`TOOLTIP_CHARS`].
fn tooltip_text(text: &str) -> SharedString {
    match text.char_indices().nth(TOOLTIP_CHARS) {
        Some((cut, _)) => format!("{}…", text.get(..cut).unwrap_or_default()).into(),
        None => SharedString::from(text.to_owned()),
    }
}

fn direction_color(event: &SessionEvent) -> Color {
    match event {
        SessionEvent::Sent(_) => Color::Accent,
        SessionEvent::Received(_) => Color::Success,
        SessionEvent::Error(_) => Color::Error,
        _ => Color::Muted,
    }
}

fn message_of(event: &SessionEvent) -> Option<&Message> {
    match event {
        SessionEvent::Sent(message) | SessionEvent::Received(message) => Some(message),
        _ => None,
    }
}

fn kind_label(kind: SessionMessageKind) -> &'static str {
    match kind {
        SessionMessageKind::Text => "Text",
        SessionMessageKind::Json => "JSON",
        SessionMessageKind::Binary => "Binary",
        SessionMessageKind::Event => "Event",
    }
}

/// What a row shows for `event`.
fn summarize(event: &SessionEvent, time_mode: TimeMode, origin: Option<u64>) -> RowSummary {
    let (glyph, direction) = match event {
        SessionEvent::Sent(_) => ("↑", "Sent"),
        SessionEvent::Received(_) => ("↓", "Received"),
        _ => ("•", "System"),
    };
    let (time, kind, size, preview) = match event {
        SessionEvent::Sent(message) | SessionEvent::Received(message) => (
            format_time(message.at, time_mode, origin),
            kind_label(message.kind).to_owned(),
            format_size(message.size),
            message_preview(message),
        ),
        SessionEvent::Connecting { target } => (
            String::new(),
            "Connecting".to_owned(),
            String::new(),
            one_line(target),
        ),
        SessionEvent::Connected { details } => {
            let mut preview = match details.response_status {
                Some(status) => format!("Connected, status {status}"),
                None => "Connected".to_owned(),
            };
            if let Some(protocol) = &details.protocol {
                preview.push_str(&format!(", protocol {protocol}"));
            }
            (
                String::new(),
                "Connected".to_owned(),
                String::new(),
                preview,
            )
        }
        SessionEvent::Error(error) => (
            String::new(),
            "Error".to_owned(),
            String::new(),
            one_line(&error.message),
        ),
        SessionEvent::Closed { reason } => (
            String::new(),
            "Closed".to_owned(),
            String::new(),
            close_text(reason),
        ),
    };
    RowSummary {
        glyph: SharedString::new_static(glyph),
        direction: SharedString::new_static(direction),
        time: time.into(),
        kind: kind.into(),
        size: size.into(),
        preview: preview.into(),
    }
}

fn format_time(at: u64, time_mode: TimeMode, origin: Option<u64>) -> String {
    match time_mode {
        TimeMode::Relative => {
            let since = at.saturating_sub(origin.unwrap_or(at));
            format!("+{}", format_duration(Duration::from_millis(since)))
        }
        TimeMode::Absolute => {
            match OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1_000_000) {
                Ok(time) => format!(
                    "{:02}:{:02}:{:02}.{:03}",
                    time.hour(),
                    time.minute(),
                    time.second(),
                    time.millisecond()
                ),
                Err(_) => String::new(),
            }
        }
    }
}

/// The payload's first line's worth, newlines folded, with a Socket.IO event's name in
/// front.
fn message_preview(message: &Message) -> String {
    let bytes = message.payload.bytes();
    let payload = if message.kind == SessionMessageKind::Binary {
        let shown = bytes.get(..BINARY_PREVIEW_BYTES).unwrap_or(bytes);
        let mut hex = HexRows::new(Arc::new(shown.to_vec()))
            .row(0)
            .map(|row| row.hex.trim_end().to_owned())
            .unwrap_or_default();
        if bytes.len() > BINARY_PREVIEW_BYTES {
            hex.push('…');
        }
        hex
    } else {
        let cut = bytes.get(..PREVIEW_CHARS * 4).unwrap_or(bytes);
        one_line(&String::from_utf8_lossy(cut))
    };
    match &message.fields {
        ProtocolFields::SocketIo { event, .. } => format!("{event} {payload}"),
        _ => payload,
    }
}

/// `text` on one line, at most [`PREVIEW_CHARS`] characters.
fn one_line(text: &str) -> String {
    let mut line: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(PREVIEW_CHARS)
        .collect();
    if text.chars().nth(PREVIEW_CHARS).is_some() {
        line.push('…');
    }
    line
}

fn close_text(reason: &CloseReason) -> String {
    match reason {
        CloseReason::Client => "Closed by the client".to_owned(),
        CloseReason::Lost => "Connection lost".to_owned(),
        CloseReason::Server { code, reason } => match code {
            Some(code) if reason.is_empty() => format!("Closed by the server, code {code}"),
            Some(code) => format!("Closed by the server, code {code}: {}", one_line(reason)),
            None => format!("Closed by the server: {}", one_line(reason)),
        },
    }
}

fn opcode_label(opcode: WebSocketOpcode) -> &'static str {
    match opcode {
        WebSocketOpcode::Continuation => "continuation",
        WebSocketOpcode::Text => "text",
        WebSocketOpcode::Binary => "binary",
        WebSocketOpcode::Close => "close",
        WebSocketOpcode::Ping => "ping",
        WebSocketOpcode::Pong => "pong",
    }
}

/// The labelled fields an expanded row shows above its payload: the protocol's own
/// fields for a message, the lifecycle details for a system event.
fn detail_fields(event: &SessionEvent) -> Vec<(SharedString, SharedString)> {
    let mut fields: Vec<(&'static str, String)> = Vec::new();
    match event {
        SessionEvent::Sent(message) | SessionEvent::Received(message) => {
            fields.push(("Size", format_size(message.size)));
            match &message.fields {
                ProtocolFields::None => {}
                ProtocolFields::SocketIo { event, ack_id } => {
                    fields.push(("Event", event.clone()));
                    fields.push((
                        "Ack ID",
                        ack_id.map_or_else(|| "none".to_owned(), |id| id.to_string()),
                    ));
                }
                ProtocolFields::Grpc { message_index } => {
                    fields.push(("Message index", message_index.to_string()));
                }
                ProtocolFields::Sse { event, id, retry } => {
                    if let Some(event) = event {
                        fields.push(("Event", event.clone()));
                    }
                    if let Some(id) = id {
                        fields.push(("ID", id.clone()));
                    }
                    if let Some(retry) = retry {
                        fields.push(("Retry", format!("{retry} ms")));
                    }
                }
                ProtocolFields::WebSocket { opcode } => {
                    fields.push(("Opcode", opcode_label(*opcode).to_owned()));
                }
            }
        }
        SessionEvent::Connecting { target } => fields.push(("Target", target.clone())),
        SessionEvent::Connected { details } => {
            if let Some(status) = details.response_status {
                fields.push(("Status", status.to_string()));
            }
            if let Some(protocol) = &details.protocol {
                fields.push(("Protocol", protocol.clone()));
            }
            for (name, value) in &details.request_headers {
                fields.push(("Request header", format!("{name}: {value}")));
            }
            for (name, value) in &details.response_headers {
                fields.push(("Response header", format!("{name}: {value}")));
            }
        }
        SessionEvent::Error(error) => fields.push(("Error", error.message.clone())),
        SessionEvent::Closed { reason } => fields.push(("Reason", close_text(reason))),
    }
    fields
        .into_iter()
        .map(|(label, value)| (SharedString::new_static(label), cut_line(&value)))
        .collect()
}

/// The detail body for a payload, and a note when it is not shown as its kind asks.
/// Heavy work: runs on a background task.
fn format_payload(
    kind: SessionMessageKind,
    bytes: Arc<Vec<u8>>,
    whole: bool,
) -> (DetailBody, Option<SharedString>) {
    if kind == SessionMessageKind::Binary {
        return (DetailBody::Hex(Arc::new(HexRows::new(bytes))), None);
    }
    if bytes.is_empty() {
        return (
            DetailBody::Lines(Arc::new(Vec::new())),
            Some(SharedString::new_static("The payload is empty.")),
        );
    }
    let mut note = None;
    let json = matches!(kind, SessionMessageKind::Json | SessionMessageKind::Event);
    if whole && json && bytes.len() > PRETTY_CAP {
        let size = format_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        note = Some(format!("Too large to pretty-print ({size}); shown as text.").into());
    } else if whole && json {
        match Formatter::Json.format(&bytes) {
            Ok(pretty) => return (text_lines(&pretty), None),
            Err(error) if kind == SessionMessageKind::Json => {
                note = Some(format!("Shown as text: {error}.").into());
            }
            Err(_) => {}
        }
    }
    (text_lines(&String::from_utf8_lossy(&bytes)), note)
}

fn text_lines(text: &str) -> DetailBody {
    DetailBody::Lines(Arc::new(text.lines().map(cut_line).collect()))
}

/// `line`, cut at [`LINE_CHARS`] with a note of how many characters were left out, so
/// no single line is shaped whole on the foreground.
/// A spilled payload whole: its in-memory `prefix`, then its file, in one buffer sized
/// once and never read past [`FULL_PAYLOAD_CAP`] however the file has grown. Blocking IO:
/// run it on a background task.
pub(crate) fn read_spilled(path: &Path, prefix: &[u8], len: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(usize::try_from(len.min(FULL_PAYLOAD_CAP)).unwrap_or(0));
    bytes.extend_from_slice(prefix);
    let held = u64::try_from(prefix.len()).unwrap_or(u64::MAX);
    std::fs::File::open(path)?
        .take(FULL_PAYLOAD_CAP.saturating_sub(held))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn cut_line(line: &str) -> SharedString {
    match line.char_indices().nth(LINE_CHARS) {
        Some((cut, _)) => {
            let kept = line.get(..cut).unwrap_or_default();
            let left_out = line.get(cut..).unwrap_or_default().chars().count();
            format!("{kept}… ({left_out} more characters)").into()
        }
        None => SharedString::from(line.to_owned()),
    }
}

#[cfg(test)]
#[path = "session_log_tests.rs"]
mod tests;
