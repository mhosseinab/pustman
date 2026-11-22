//! The Console panel (REQ-CON-02, REQ-CON-03, REQ-UI-04): the bottom-dock view over
//! [`ConsoleStore`] — a virtualized entry list with level, protocol and text filters,
//! clear and pause, and the raw request/response of the expanded entry (REQ-CON-03).
//!
//! `Decision:` the panel answers `pustman_actions::console::Clear` and
//! `pustman_actions::console::Pause` under the `"Console"` key context. The plan's
//! `console::Focus` does not exist in `pustman_actions` and that crate is outside this
//! step's file set, so focus lands through the dock's selector entry instead; and
//! `workspace::ToggleConsole` stays answered by `Workspace::toggle_console` alone — a
//! second handler here would fight it over the dock's visibility.
//!
//! `Decision:` every list row is the same height (GPUI's `uniform_list` lays rows out at
//! one measured height), so the expanded entry's raw text renders in a details region
//! below the list instead of growing one row.
//!
//! `Decision:` the level filter is one segmented control — [`ui::TabBar::segmented()`],
//! the switch `response_viewer`'s Pretty/Raw/Hex/Preview control draws — and Pause is an
//! icon button whose tooltip is its name, not four text chips and a text button (S02/R02
//! for the workbench control, S26/R26 for the selected state and the icon-only control's
//! accessible name; #232). `panel_toolbar_tests.rs` pins both.
//!
//! `Decision:` [`ConsoleEntry`] carries no id and the store is outside this step, so a
//! row's element id is its content fingerprint plus its occurrence among identical rows —
//! never its row position, which an eviction from the ring buffer would shift (the
//! `history_panel` row-id rule). The id is also what the row's focus handle hangs off, so
//! keyboard focus follows its entry through pushes, evictions and filter changes.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, ElementId, Entity, FocusHandle, Focusable, Role, Subscription};
use pustman_actions::console::{Clear, Pause};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;
use workspace::{DockPosition, Panel};

use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// The panel's dock id and the key context its actions dispatch under.
const PANEL_ID: &str = "console";
const CONSOLE_CONTEXT: &str = "Console";
/// The key context `ui`'s `ACTIVATION_CONTEXT` names (a `pub(crate)` const there): the
/// controls that activate on Enter or Space.
const ACTIVATION_CONTEXT: &str = "Button";
/// One list row and the details region are fixed sizes so `uniform_list` keeps one row
/// height and the panel never jumps when an entry expands.
const ROW_HEIGHT_REMS: f32 = 1.75;
const DETAILS_HEIGHT_REMS: f32 = 14.;
const FILTER_WIDTH_REMS: f32 = 14.;
/// Debug selectors the layout tests measure the drawn frame through.
pub(crate) const TEXT_FILTER_SELECTOR: &str = "console-text-filter";
pub(crate) const ENTRIES_AREA_SELECTOR: &str = "console-entries-area";
pub(crate) const EMPTY_STATE_SELECTOR: &str = "console-empty-state";
pub(crate) const ENTRY_ROW_SELECTOR: &str = "console-entry-row";
pub(crate) const DETAILS_SELECTOR: &str = "console-details";
/// Debug selectors the notice tests measure the details sections through: one per
/// section title, so the pane's sections can be told apart from a drawn frame.
pub(crate) const NOTICE_SECTION_SELECTOR: &str = "console-details-notice";
pub(crate) const REQUEST_SECTION_SELECTOR: &str = "console-details-request";
pub(crate) const RESPONSE_SECTION_SELECTOR: &str = "console-details-response";
/// Debug selectors the toolbar tests measure the icon buttons through.
const PAUSE_CONTROL: &str = "console-pause-control";
const CLEAR_CONTROL: &str = "console-clear-control";
/// The level filter's segments, in bar order: each segment's id and label, and the level
/// it filters by — `None` is "All", which shows every level (REQ-CON-02).
const LEVEL_FILTERS: [(&str, &str, Option<ConsoleLevel>); 4] = [
    ("console-filter-all", "All", None),
    ("console-filter-info", "Info", Some(ConsoleLevel::Info)),
    (
        "console-filter-warning",
        "Warning",
        Some(ConsoleLevel::Warning),
    ),
    ("console-filter-error", "Error", Some(ConsoleLevel::Error)),
];

/// The Console panel: the entry list, its filters and the expanded raw view.
pub struct ConsolePanel {
    focus_handle: FocusHandle,
    level_filter: Option<ConsoleLevel>,
    protocol_filter: Option<SharedString>,
    text_filter: Entity<TextInput>,
    /// The level filter: one segmented control with one selection (S02/R02).
    level_bar: Entity<TabBar>,
    /// The buffer index of the entry whose raw text the details region shows.
    expanded: Option<usize>,
    /// That entry's content fingerprint ([`entry_fingerprint`]). A ring-buffer eviction
    /// shifts every buffer index, so [`Self::track_expanded_entry`] keeps the two in
    /// step: the expansion follows its own entry and is dropped once the entry is gone
    /// (#135).
    expanded_fingerprint: Option<u64>,
    /// One focus handle per visible row key (the `ui::Button` per-id focus shape): rows
    /// are tab stops, and a handle follows its key, so focus survives pushes that evict
    /// the oldest entry. Kept fresh by [`Self::sync_row_focus`], never by render.
    row_focus: HashMap<String, FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl ConsolePanel {
    /// A panel over the store's entries, ready to be registered in the bottom dock.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text_filter =
            cx.new(|cx| TextInput::new(window, cx).with_placeholder("Filter console entries"));
        let level_bar = cx.new(|cx| TabBar::new(cx).segmented().with_tabs(level_tabs()));
        let subscriptions = vec![
            cx.observe_global::<ConsoleStore>(|this, cx| this.refresh(cx)),
            cx.subscribe(&text_filter, |this, _input, _: &TextInputEvent, cx| {
                // The rows follow the field's text, so any change re-filters.
                this.refresh(cx);
            }),
            cx.subscribe(&level_bar, Self::on_level_bar_event),
        ];
        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            level_filter: None,
            protocol_filter: None,
            text_filter,
            level_bar,
            expanded: None,
            expanded_fingerprint: None,
            row_focus: HashMap::new(),
            _subscriptions: subscriptions,
        };
        panel.sync_row_focus(cx);
        panel
    }

    /// Show only entries of `level` — `None` shows every level. What the level segments
    /// call.
    pub fn set_level_filter(&mut self, level: Option<ConsoleLevel>, cx: &mut Context<Self>) {
        self.level_filter = level;
        self.clear_expansion();
        self.sync_level_bar(cx);
        self.refresh(cx);
    }

    /// The level bar's events: clicking a segment, or moving onto one with the arrows,
    /// sets the filter. An event that no longer describes the bar's selection is a stale
    /// echo and is dropped (`response_viewer`'s mode bar does the same).
    fn on_level_bar_event(
        &mut self,
        level_bar: Entity<TabBar>,
        event: &TabBarEvent,
        cx: &mut Context<Self>,
    ) {
        let index = match event {
            TabBarEvent::Selected { index } | TabBarEvent::Activated { index } => *index,
        };
        if index != level_bar.read(cx).selected() {
            return;
        }
        let Some((_, _, level)) = LEVEL_FILTERS.get(index) else {
            return;
        };
        self.set_level_filter(*level, cx);
    }

    /// Keep the bar's selection on the filter, so a filter set from code never leaves
    /// the control showing another segment. While the selection already agrees this is a
    /// no-op, which is also what stops the bar's own echo at one hop.
    fn sync_level_bar(&mut self, cx: &mut Context<Self>) {
        let selected = LEVEL_FILTERS
            .iter()
            .position(|(_, _, level)| *level == self.level_filter)
            .unwrap_or(0);
        self.level_bar.update(cx, |level_bar, cx| {
            if level_bar.selected() != selected {
                level_bar.set_selected(selected, cx);
            }
        });
    }

    /// Show only entries of `protocol` — `None` shows every protocol. What the protocol
    /// chip calls; v1 logs `http` only (plan D7).
    pub fn set_protocol_filter(&mut self, protocol: Option<SharedString>, cx: &mut Context<Self>) {
        self.protocol_filter = protocol;
        self.clear_expansion();
        self.refresh(cx);
    }

    /// The buffer indices of the entries the list shows right now, oldest first.
    pub fn visible_entries(&self, cx: &App) -> Vec<usize> {
        visible_entries(
            cx.global::<ConsoleStore>().entries(),
            self.level_filter,
            self.protocol_filter.as_deref(),
            &self.text_filter.read(cx).text(),
        )
    }

    /// Every visible row as its entry's buffer index and its stable row key, in row
    /// order: the entry's content fingerprint plus its occurrence among identical
    /// visible rows (the `history_panel` rule — a row id never encodes a row position).
    /// The key keeps hover and keyboard focus on the same entry across pushes, evictions
    /// and filter changes.
    fn visible_rows(&self, cx: &App) -> Vec<(usize, String)> {
        let entries = cx.global::<ConsoleStore>().entries();
        let mut occurrences: HashMap<u64, usize> = HashMap::new();
        self.visible_entries(cx)
            .into_iter()
            .map(|entry_index| {
                let key = match entries.get(entry_index) {
                    Some(entry) => {
                        let fingerprint = entry_fingerprint(entry);
                        let occurrence = occurrences.entry(fingerprint).or_insert(0);
                        let key = format!("console-entry-{fingerprint:x}-{occurrence}");
                        *occurrence += 1;
                        key
                    }
                    None => format!("console-entry-gone-{entry_index}"),
                };
                (entry_index, key)
            })
            .collect()
    }

    /// Keep one focus handle per visible row key (the `ui::Button` per-id focus shape),
    /// dropping handles for rows no longer shown. Never called from render, which must
    /// stay a pure read of state.
    fn sync_row_focus(&mut self, cx: &mut App) {
        let keys: HashSet<String> = self
            .visible_rows(cx)
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        self.row_focus.retain(|key, _| keys.contains(key));
        for key in keys {
            self.row_focus
                .entry(key)
                .or_insert_with(|| cx.focus_handle());
        }
    }

    /// Re-filter and redraw. Every place that changes what the list shows funnels here,
    /// so the rows' focus handles are never stale when a frame draws them.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.track_expanded_entry(cx);
        self.sync_row_focus(cx);
        cx.notify();
    }

    /// Drop the expansion entirely.
    fn clear_expansion(&mut self) {
        self.expanded = None;
        self.expanded_fingerprint = None;
    }

    /// Keep the expansion on its own entry as the buffer changes (#135): a push over the
    /// cap evicts from the front and shifts every buffer index, so the stored index is
    /// re-pointed at the entry's new place and dropped when the entry itself is gone —
    /// never left showing whatever now sits at the old index.
    fn track_expanded_entry(&mut self, cx: &App) {
        let Some(fingerprint) = self.expanded_fingerprint else {
            return;
        };
        let entries = cx.global::<ConsoleStore>().entries();
        let at_index = self
            .expanded
            .and_then(|entry_index| entries.get(entry_index))
            .is_some_and(|entry| entry_fingerprint(entry) == fingerprint);
        if at_index {
            return;
        }
        match entries
            .iter()
            .position(|entry| entry_fingerprint(entry) == fingerprint)
        {
            Some(entry_index) => self.expanded = Some(entry_index),
            None => self.clear_expansion(),
        }
    }

    fn clear_entries(&mut self, _: &Clear, _window: &mut Window, cx: &mut Context<Self>) {
        self.clear(cx);
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.clear_expansion();
        ConsoleStore::clear(cx);
        self.refresh(cx);
    }

    fn toggle_pause(&mut self, _: &Pause, _window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_paused(cx);
    }

    fn toggle_paused(&mut self, cx: &mut Context<Self>) {
        let paused = cx.global::<ConsoleStore>().is_paused();
        ConsoleStore::set_paused(cx, !paused);
        cx.notify();
    }

    fn toggle_expanded(&mut self, entry_index: usize, cx: &mut Context<Self>) {
        if self.expanded == Some(entry_index) {
            self.clear_expansion();
        } else {
            self.expanded = Some(entry_index);
            self.expanded_fingerprint = cx
                .global::<ConsoleStore>()
                .entries()
                .get(entry_index)
                .map(entry_fingerprint);
        }
        cx.notify();
    }

    /// One list row: the summary line (REQ-CON-01) — level, method, status, URL, time
    /// and size — or, for a feed notice (no raw request of its own), its text. The row is
    /// a control (`ui::Button`'s shape): a tab stop per row key that Enter or Space
    /// activates like a click, so the raw details expand without a mouse.
    fn render_row(&self, entry_index: usize, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = cx.global::<ConsoleStore>().entries().get(entry_index) else {
            // The ring buffer dropped the entry between filtering and drawing.
            return div().into_any_element();
        };
        let expanded = self.expanded == Some(entry_index);
        let focus_handle = self
            .row_focus
            .get(key)
            .map(|handle| handle.clone().tab_stop(true));
        let row = h_flex()
            .id(ElementId::NamedChild(
                Arc::new(ElementId::Name(SharedString::from(key.to_string()))),
                SharedString::new_static("details-toggle"),
            ))
            .key_context(ACTIVATION_CONTEXT)
            .role(Role::Button)
            .aria_label("Expand request and response details")
            .aria_toggled(expanded.into())
            .debug_selector(|| ENTRY_ROW_SELECTOR.to_owned())
            .w_full()
            .h(rems(ROW_HEIGHT_REMS))
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base08.rems(cx))
            // The border is always drawn (transparent) so the focus ring shows without
            // moving the row's layout.
            .border_1()
            .border_color(cx.theme().colors().border_transparent)
            .focus_visible(|row| row.border_color(cx.theme().colors().border_focused))
            .cursor_pointer()
            .when(expanded, |row| row.bg(cx.theme().colors().element_selected))
            .hover(|row| row.bg(cx.theme().colors().element_hover))
            .map(|row| match focus_handle.as_ref() {
                // The `ui::Button` shape: with a handle the handle's own tab stop decides
                // the tab order, and GPUI turns an Enter or Space press on the focused row
                // into the same click a mouse makes.
                Some(handle) => row.track_focus(handle),
                None => row.focusable().tab_stop(true),
            })
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_expanded(entry_index, cx)))
            .child(
                Icon::new(level_icon(entry.level))
                    .size(IconSize::Small)
                    .color(level_color(entry.level)),
            );
        if let Some(notice) = entry.notice() {
            return row
                .child(
                    Label::new(notice.to_owned())
                        .color(level_color(entry.level))
                        .truncate(),
                )
                .into_any_element();
        }
        row.child(
            Label::new(
                entry
                    .method
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| entry.protocol.clone()),
            )
            .color(Color::Muted),
        )
        .children(
            entry
                .status
                .map(|status| Label::new(status.to_string()).color(status_color(status))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Label::new(entry.url.clone()).truncate()),
        )
        .child(Label::new(format_time(entry.time)).color(Color::Muted))
        .child(Label::new(format_size(entry.size)).color(Color::Muted))
        .into_any_element()
    }

    /// The raw text of the expanded entry (REQ-CON-03) below the list — or a notice's
    /// message under its own `Notice` section, never under a response's (#115).
    fn render_details(&self, cx: &App) -> AnyElement {
        let Some(entry) = self
            .expanded
            .and_then(|entry_index| cx.global::<ConsoleStore>().entries().get(entry_index))
        else {
            return div().into_any_element();
        };
        v_flex()
            .id(SharedString::new_static("console-details"))
            .debug_selector(|| DETAILS_SELECTOR.to_owned())
            .h(rems(DETAILS_HEIGHT_REMS))
            .overflow_y_scroll()
            .gap(DynamicSpacing::Base04.rems(cx))
            .p(DynamicSpacing::Base08.rems(cx))
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .children(
                (!entry.raw_request.is_empty()).then(|| {
                    raw_section("Request", REQUEST_SECTION_SELECTOR, &entry.raw_request, cx)
                }),
            )
            .map(|details| match entry.notice() {
                // A notice is a message the entry carries, not wire text: it gets its own
                // section and is never drawn as a response the wire never sent (#115).
                Some(notice) => {
                    details.child(raw_section("Notice", NOTICE_SECTION_SELECTOR, notice, cx))
                }
                None => details.children((!entry.raw_response.is_empty()).then(|| {
                    raw_section(
                        "Response",
                        RESPONSE_SECTION_SELECTOR,
                        &entry.raw_response,
                        cx,
                    )
                })),
            })
            .into_any_element()
    }

    /// The filter row: the level segments of one segmented control, the protocol chip,
    /// the text filter, and pause and clear on the right, with the Paused state called
    /// out while it is on.
    fn render_toolbar(&self, paused: bool, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base08.rems(cx))
            .py(DynamicSpacing::Base04.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(self.level_bar.clone())
            .child(
                Button::new("console-filter-protocol", "HTTP")
                    .style(chip_style(self.protocol_filter.is_some()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let filter = if this.protocol_filter.is_some() {
                            None
                        } else {
                            Some(SharedString::new_static("http"))
                        };
                        this.set_protocol_filter(filter, cx);
                    })),
            )
            .child(
                div()
                    .debug_selector(|| TEXT_FILTER_SELECTOR.to_owned())
                    .w(rems(FILTER_WIDTH_REMS))
                    .px(DynamicSpacing::Base03.rems(cx))
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .child(self.text_filter.clone()),
            )
            .child(div().flex_1())
            .children(paused.then(|| {
                Label::new("Paused")
                    .size(LabelSize::Small)
                    .color(Color::Warning)
            }))
            .child(
                div().debug_selector(|| PAUSE_CONTROL.to_owned()).child(
                    IconButton::new(
                        "console-pause",
                        if paused {
                            IconName::Play
                        } else {
                            IconName::Pause
                        },
                        Tooltip::text(if paused {
                            "Resume console"
                        } else {
                            "Pause console"
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_paused(cx))),
                ),
            )
            .child(
                div().debug_selector(|| CLEAR_CONTROL.to_owned()).child(
                    IconButton::new(
                        "console-clear",
                        IconName::Trash,
                        Tooltip::text("Clear console"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
                ),
            )
            .into_any_element()
    }
}

impl Render for ConsolePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let paused = cx.global::<ConsoleStore>().is_paused();
        // `VirtualList`'s row-key and row closures run without `self` and without the
        // store, so the (entry index, row key) pairs of the visible rows are snapshotted
        // here (the shape `command_palette`'s picker uses).
        let rows: Rc<[(usize, String)]> = self.visible_rows(cx).into();
        let rows_for_keys = Rc::clone(&rows);
        let rows_for_render = Rc::clone(&rows);
        let list = VirtualList::new(
            ElementId::from(&self.focus_handle),
            rows.len(),
            move |row| match rows_for_keys.get(row) {
                Some((_, key)) => ElementId::Name(SharedString::from(key.clone())),
                None => ElementId::Name(SharedString::new_static("console-entry-gone")),
            },
            cx.processor(
                move |this, row: usize, _window, cx| match rows_for_render.get(row) {
                    Some((entry_index, key)) => this.render_row(*entry_index, key, cx),
                    None => div().into_any_element(),
                },
            ),
        );
        v_flex()
            .id(PANEL_ID)
            .track_focus(&self.focus_handle)
            .key_context(CONSOLE_CONTEXT)
            .on_action(cx.listener(Self::clear_entries))
            .on_action(cx.listener(Self::toggle_pause))
            .size_full()
            .font_ui(cx)
            .child(self.render_toolbar(paused, cx))
            .child(
                v_flex()
                    .debug_selector(|| ENTRIES_AREA_SELECTOR.to_owned())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .map(|container| {
                        if rows.is_empty() {
                            container.child(
                                v_flex().flex_1().items_center().justify_center().child(
                                    div()
                                        .debug_selector(|| EMPTY_STATE_SELECTOR.to_owned())
                                        .child(
                                            Label::new("No console entries")
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                ),
                            )
                        } else {
                            container.child(list)
                        }
                    }),
            )
            .child(self.render_details(cx))
    }
}

impl Focusable for ConsolePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ConsolePanel {
    fn panel_id(&self) -> &'static str {
        PANEL_ID
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Console")
    }

    fn icon(&self) -> IconName {
        IconName::Terminal
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Bottom
    }
}

/// The buffer indices of the entries matching every filter, oldest first: the level and
/// protocol match exactly (case-insensitively for the protocol), and the text is a
/// case-insensitive match on the URL, the method and the raw request and response text.
fn visible_entries(
    entries: &VecDeque<ConsoleEntry>,
    level: Option<ConsoleLevel>,
    protocol: Option<&str>,
    text: &str,
) -> Vec<usize> {
    let query = text.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            level.is_none_or(|level| entry.level == level)
                && protocol.is_none_or(|protocol| entry.protocol.eq_ignore_ascii_case(protocol))
                && (query.is_empty() || entry_matches_text(entry, &query))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Whether `query` (already lowercased) appears in the entry's URL, method or raw text.
fn entry_matches_text(entry: &ConsoleEntry, query: &str) -> bool {
    entry.url.to_lowercase().contains(query)
        || entry
            .method
            .as_ref()
            .is_some_and(|method| method.to_string().to_lowercase().contains(query))
        || entry.raw_request.to_lowercase().contains(query)
        || entry.raw_response.to_lowercase().contains(query)
}

/// The content fingerprint behind one row's key. Identical entries — the same request
/// sent twice — hash the same, so [`ConsolePanel::visible_rows`] pairs the fingerprint
/// with the entry's occurrence among identical rows to keep keys unique.
fn entry_fingerprint(entry: &ConsoleEntry) -> u64 {
    let mut hasher = DefaultHasher::new();
    entry.url.hash(&mut hasher);
    entry.protocol.hash(&mut hasher);
    entry
        .method
        .as_ref()
        .map(ToString::to_string)
        .hash(&mut hasher);
    entry.raw_request.hash(&mut hasher);
    entry.raw_response.hash(&mut hasher);
    entry.status.hash(&mut hasher);
    hasher.finish()
}

/// The raw request or response block of the expanded entry, or a notice's message under
/// its own section title.
fn raw_section(title: &'static str, selector: &'static str, raw: &str, cx: &App) -> AnyElement {
    v_flex()
        .gap(DynamicSpacing::Base02.rems(cx))
        .debug_selector(|| selector.to_owned())
        .child(Label::new(title).size(LabelSize::Small).color(Color::Muted))
        .child(
            div()
                .font_buffer(cx)
                .text_ui_sm(cx)
                .child(SharedString::from(raw.to_string())),
        )
        .into_any_element()
}

fn chip_style(active: bool) -> ButtonStyle {
    if active {
        ButtonStyle::Tinted(TintColor::Accent)
    } else {
        ButtonStyle::Subtle
    }
}

/// The level filter's segments as the bar draws them.
fn level_tabs() -> Vec<Tab> {
    LEVEL_FILTERS
        .into_iter()
        .map(|(id, label, _level)| Tab::new(id, label))
        .collect()
}

fn level_icon(level: ConsoleLevel) -> IconName {
    match level {
        ConsoleLevel::Info => IconName::Info,
        ConsoleLevel::Warning => IconName::TriangleAlert,
        ConsoleLevel::Error => IconName::CircleX,
    }
}

fn level_color(level: ConsoleLevel) -> Color {
    match level {
        ConsoleLevel::Info => Color::Muted,
        ConsoleLevel::Warning => Color::Warning,
        ConsoleLevel::Error => Color::Error,
    }
}

fn status_color(status: u16) -> Color {
    match status {
        200..=299 => Color::Success,
        300..=399 => Color::Warning,
        400..=599 => Color::Error,
        _ => Color::Muted,
    }
}

/// The exchange time as the summary line shows it: blank when the entry carries none, in
/// the shared `ui` form every pane shows a duration in (#134).
fn format_time(time: Option<Duration>) -> String {
    time.map_or_else(String::new, ui::format_duration)
}

/// The response size as the summary line shows it: blank when the entry carries none, in
/// the shared `ui` form every pane shows a size in ([`format_time`]'s size twin, #134).
fn format_size(size: Option<u64>) -> String {
    size.map_or_else(String::new, ui::format_size)
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_format_tests.rs"]
mod format_tests;

#[cfg(test)]
#[path = "panel_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "panel_row_height_tests.rs"]
mod row_height_tests;

#[cfg(test)]
#[path = "panel_expanded_tests.rs"]
mod expanded_tests;

#[cfg(test)]
#[path = "panel_notice_tests.rs"]
mod notice_tests;

#[cfg(test)]
#[path = "panel_toolbar_tests.rs"]
mod toolbar_tests;
