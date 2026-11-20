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

/// The Console panel: the entry list, its filters and the expanded raw view.
pub struct ConsolePanel {
    focus_handle: FocusHandle,
    level_filter: Option<ConsoleLevel>,
    protocol_filter: Option<SharedString>,
    text_filter: Entity<TextInput>,
    /// The buffer index of the entry whose raw text the details region shows.
    expanded: Option<usize>,
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
        let subscriptions = vec![
            cx.observe_global::<ConsoleStore>(|this, cx| this.refresh(cx)),
            cx.subscribe(&text_filter, |this, _input, _: &TextInputEvent, cx| {
                // The rows follow the field's text, so any change re-filters.
                this.refresh(cx);
            }),
        ];
        let mut panel = Self {
            focus_handle: cx.focus_handle(),
            level_filter: None,
            protocol_filter: None,
            text_filter,
            expanded: None,
            row_focus: HashMap::new(),
            _subscriptions: subscriptions,
        };
        panel.sync_row_focus(cx);
        panel
    }

    /// Show only entries of `level` — `None` shows every level. What the level chips
    /// call.
    pub fn set_level_filter(&mut self, level: Option<ConsoleLevel>, cx: &mut Context<Self>) {
        self.level_filter = level;
        self.expanded = None;
        self.refresh(cx);
    }

    /// Show only entries of `protocol` — `None` shows every protocol. What the protocol
    /// chip calls; v1 logs `http` only (plan D7).
    pub fn set_protocol_filter(&mut self, protocol: Option<SharedString>, cx: &mut Context<Self>) {
        self.protocol_filter = protocol;
        self.expanded = None;
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
        self.sync_row_focus(cx);
        cx.notify();
    }

    fn clear_entries(&mut self, _: &Clear, _window: &mut Window, cx: &mut Context<Self>) {
        self.clear(cx);
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.expanded = None;
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
        self.expanded = if self.expanded == Some(entry_index) {
            None
        } else {
            Some(entry_index)
        };
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
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_REMS))
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
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
        if is_notice(entry) {
            return row
                .child(
                    Label::new(entry.raw_response.clone())
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

    /// The raw text of the expanded entry (REQ-CON-03), below the list.
    fn render_details(&self, cx: &App) -> AnyElement {
        let Some(entry) = self
            .expanded
            .and_then(|entry_index| cx.global::<ConsoleStore>().entries().get(entry_index))
        else {
            return div().into_any_element();
        };
        v_flex()
            .id(SharedString::new_static("console-details"))
            .h(px_to_rems(DETAILS_HEIGHT_REMS))
            .overflow_y_scroll()
            .gap(DynamicSpacing::Base04.rems(cx))
            .p(DynamicSpacing::Base04.rems(cx))
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .children(
                (!entry.raw_request.is_empty())
                    .then(|| raw_section("Request", &entry.raw_request, cx)),
            )
            .children(
                (!entry.raw_response.is_empty())
                    .then(|| raw_section("Response", &entry.raw_response, cx)),
            )
            .into_any_element()
    }

    /// The filter row: level chips, the protocol chip, the text filter, and pause and
    /// clear on the right, with the Paused state called out while it is on.
    fn render_toolbar(&self, paused: bool, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(self.level_chip("All", None, cx))
            .child(self.level_chip("Info", Some(ConsoleLevel::Info), cx))
            .child(self.level_chip("Warning", Some(ConsoleLevel::Warning), cx))
            .child(self.level_chip("Error", Some(ConsoleLevel::Error), cx))
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
                Button::new("console-pause", if paused { "Resume" } else { "Pause" })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_paused(cx))),
            )
            .child(
                IconButton::new(
                    "console-clear",
                    IconName::Trash,
                    Tooltip::text("Clear console"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            )
            .into_any_element()
    }

    /// One level chip: highlighted while its level is the filter. `level` is `None` for
    /// "All".
    fn level_chip(
        &self,
        label: &'static str,
        level: Option<ConsoleLevel>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.level_filter == level;
        Button::new(
            match level {
                None => "console-filter-all",
                Some(ConsoleLevel::Info) => "console-filter-info",
                Some(ConsoleLevel::Warning) => "console-filter-warning",
                Some(ConsoleLevel::Error) => "console-filter-error",
            },
            label,
        )
        .style(chip_style(active))
        .on_click(cx.listener(move |this, _, _, cx| this.set_level_filter(level, cx)))
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

/// A feed notice: no request of its own, only its text (S32b). It is drawn as that text
/// rather than as an empty request row.
fn is_notice(entry: &ConsoleEntry) -> bool {
    entry.raw_request.is_empty() && !entry.raw_response.is_empty()
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

/// The raw request or response block of the expanded entry.
fn raw_section(title: &'static str, raw: &str, cx: &App) -> AnyElement {
    v_flex()
        .gap(DynamicSpacing::Base02.rems(cx))
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

/// The exchange time as the summary line shows it: milliseconds under a second, one
/// decimal above. Integer math throughout — response times are user data.
fn format_time(time: Option<Duration>) -> String {
    let Some(time) = time else {
        return String::new();
    };
    let millis = time.as_millis();
    if millis < 1000 {
        format!("{millis} ms")
    } else {
        format!("{}.{} s", millis / 1000, (millis % 1000) / 100)
    }
}

/// The response size as the summary line shows it. Integer math throughout — response
/// sizes are user data, so nothing here casts one.
fn format_size(size: Option<u64>) -> String {
    let Some(size) = size else {
        return String::new();
    };
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    match size {
        0..KIB => format!("{size} B"),
        KIB..MIB => format!("{}.{} KB", size / KIB, (size % KIB) * 10 / KIB),
        _ => format!("{}.{} MB", size / MIB, (size % MIB) * 10 / MIB),
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_layout_tests.rs"]
mod layout_tests;
