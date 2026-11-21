//! The History panel's list (PRD §8.5, REQ-HIST-01/04): the recorded sends of one
//! workspace, grouped by day and filterable through a search box. Activating a row — a
//! click on it, or Enter on the keyboard selection — opens the entry through
//! [`crate::open_entry`] (REQ-HIST-02).
//!
//! `storage` is synchronous (ADR-0002), so every read runs on `cx.background_executor`
//! and the foreground never touches SQLite. The read is kicked off by the constructor
//! and re-run on every search edit; the [`Task`] is stored, so a new query cancels the
//! read it supersedes and only the newest result can land. Rows carry method, target,
//! status and time (REQ-HIST-01) — never request or response content (REQ-HIST-03).

use std::sync::{Arc, Mutex};

use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, Focusable, Global,
    InteractiveElement, IntoElement, ParentElement, Render, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, UniformListScrollHandle, Window, div,
};
use storage::{HistoryEntry, LocalStore};
use text_input::{TextInput, TextInputEvent};
use time::{Date, OffsetDateTime};
use ui::prelude::*;
use workspace::{DockPosition, Panel, Workspace};

use crate::open_entry::open_entry;

#[cfg(test)]
#[path = "history_list_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "history_list_activation_tests.rs"]
mod activation_tests;

/// The panel's stable id: its dock selector entry, its hitbox and the id
/// [`crate::focus_history_panel`] activates it under.
pub const PANEL_ID: &str = "history";

/// The key context the list answers the keyboard in: the arrows move the selection,
/// Enter opens it. The search field's own `"TextInput"` context nests inside this one —
/// the `command_palette::picker` shape.
pub const LIST_KEY_CONTEXT: &str = "HistoryList";

/// Marks that this app's keymap already carries [`LIST_KEY_CONTEXT`]'s bindings.
/// [`HistoryList::new`] is the only door this crate has to bind them (the crate's `init`
/// is outside this step's scope), and a second panel must not stack duplicates.
struct ListKeysBound;

impl Global for ListKeysBound {}

/// One row's height at the default scale. Rows are uniform because the virtualized
/// list measures one row for all of them — the same contract `command_palette::Picker`
/// draws its match rows under.
const ROW_HEIGHT_PX: f32 = 40.;

/// The width of a row's method column: fixed so the targets line up down the list.
const METHOD_COLUMN_PX: f32 = 48.;

/// The one message a failed read shows. `storage`'s error text can quote the row it
/// failed on, and rows are request content (PRD §7, REQ-HIST-03) — the failure is
/// folded into this fixed message and the error is dropped unread.
const LOAD_FAILURE_MESSAGE: &str = "The request history could not be loaded";

/// One row of the list: a day header or one recorded send.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryRow {
    /// The header opening one UTC day's group — the day `storage`'s `history` rows are
    /// bucketed by ([`storage::HistoryRepo::list_by_day`]).
    Day { date: Date, label: SharedString },
    /// One recorded send, newest first within its day. Boxed: an entry carries its
    /// stored request JSON, so the rows' day headers must not pay for it.
    Entry(Box<HistoryEntry>),
}

/// The left-dock History panel: the search box over the day-grouped list of a
/// workspace's history.
pub struct HistoryList {
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
    search: Entity<TextInput>,
    /// The rows of the last successful read, grouped and ordered for drawing. Read-only
    /// after a read lands, so rendering just clones the handle.
    rows: Arc<Vec<HistoryRow>>,
    /// Until the first read lands there is nothing to say about the history yet — the
    /// empty state must not flash "No requests sent yet" over a load in flight.
    is_loading: bool,
    /// The fixed message of the last failed read, shown inline above the list.
    error: Option<SharedString>,
    /// The in-flight read. Replacing it cancels the previous one (a dropped `Task`
    /// cancels its work — AGENTS.md), so only the newest query's rows can land.
    pending_load: Option<Task<()>>,
    /// The entry the keyboard is on — its stored id, which survives search re-ordering
    /// — or [`None`] while the arrows and clicks have not put it anywhere. Rows the
    /// store left without an id cannot carry it (none reach the list from `storage`).
    selected_entry_id: Option<i64>,
    /// The list's scroll state, so moving the selection scrolls its row into view.
    scroll_handle: UniformListScrollHandle,
    // Kept so the search box's edits keep reaching us; dropping it would silently stop
    // filtering.
    _search_subscription: Subscription,
}

impl HistoryList {
    /// The panel showing `workspace_id`'s history through `store`. It opens on the full
    /// newest-first list (the `""` query) and re-reads on every search edit.
    pub fn new(
        store: Arc<Mutex<LocalStore>>,
        workspace_id: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // `Decision:` the list's keys are bound here rather than in `history_panel::init`
        // (outside this step's scope) and reuse `ui`'s component actions instead of
        // declaring new ones — `command_palette::picker`'s shape with the actions that
        // already exist. They go in once per app: a second panel must not stack
        // duplicates into the keymap.
        if cx.try_global::<ListKeysBound>().is_none() {
            cx.set_global(ListKeysBound);
            cx.bind_keys([
                gpui::KeyBinding::new("down", SelectNext, Some(LIST_KEY_CONTEXT)),
                gpui::KeyBinding::new("up", SelectPrevious, Some(LIST_KEY_CONTEXT)),
                gpui::KeyBinding::new("enter", Activate, Some(LIST_KEY_CONTEXT)),
            ]);
        }
        let search = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static("Search history"))
        });
        let _search_subscription = cx.subscribe_in(&search, window, Self::on_search_event);
        let mut this = Self {
            store,
            workspace_id: workspace_id.into(),
            search,
            rows: Arc::new(Vec::new()),
            is_loading: true,
            error: None,
            pending_load: None,
            selected_entry_id: None,
            scroll_handle: UniformListScrollHandle::new(),
            _search_subscription,
        };
        this.reload(cx);
        this
    }

    /// The rows the list draws — the last read's entries under their day headers — so a
    /// test can assert the grouping the view exposes.
    pub fn rows(&self) -> &[HistoryRow] {
        self.rows.as_slice()
    }

    /// The search box is the panel's only input: every edit re-reads the history under
    /// the new query, and Enter in it opens the selected row — the panel's whole
    /// keyboard contract lives in its one focus (the `command_palette::picker` shape).
    fn on_search_event(
        &mut self,
        _search: &Entity<TextInput>,
        event: &TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Changed => self.reload(cx),
            TextInputEvent::Submitted => self.open_selected(window, cx),
        }
    }

    /// The row the keyboard selection is on: the row carrying [`Self::selected_entry_id`],
    /// wherever the latest read placed it — so a search re-run leaves the selection on
    /// the same entry (or clears it) instead of sliding onto its neighbour.
    fn selected_row_index(&self) -> Option<usize> {
        let selected = self.selected_entry_id?;
        self.rows
            .iter()
            .position(|row| matches!(row, HistoryRow::Entry(entry) if entry.id == Some(selected)))
    }

    /// Move the keyboard selection one entry row, skipping the day headers and stopping
    /// at the ends: the history is a long list, where wrapping would fling the viewport
    /// from one end to the other. With no selection yet, Down lands on the first entry
    /// row and Up on the last.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let rows = Arc::clone(&self.rows);
        let entry_rows: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| matches!(row, HistoryRow::Entry(_)).then_some(index))
            .collect();
        if entry_rows.is_empty() {
            return;
        }
        let current = self
            .selected_row_index()
            .and_then(|selected| entry_rows.iter().position(|&index| index == selected));
        let target = match (current, forward) {
            (Some(position), true) => (position + 1).min(entry_rows.len() - 1),
            (Some(position), false) => position.saturating_sub(1),
            (None, true) => 0,
            (None, false) => entry_rows.len() - 1,
        };
        let Some(&target_row) = entry_rows.get(target) else {
            return;
        };
        self.selected_entry_id = match rows.get(target_row) {
            Some(HistoryRow::Entry(entry)) => entry.id,
            _ => None,
        };
        self.scroll_handle
            .scroll_to_item(target_row, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(true, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(false, cx);
    }

    /// Enter opens the selected row — and does nothing at all while the keyboard is on
    /// no row (the `command_palette::picker`'s empty-selection contract).
    fn activate(&mut self, _: &Activate, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selected(window, cx);
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected_row_index() else {
            return;
        };
        let rows = Arc::clone(&self.rows);
        let Some(HistoryRow::Entry(entry)) = rows.get(index) else {
            return;
        };
        self.open_row((**entry).clone(), window, cx);
    }

    /// The one door a row is activated through — a click on it, and Enter on the
    /// selection: select the entry, then open it in the center pane through
    /// [`crate::open_entry`], which owns the restore and its error toasts.
    ///
    /// `Decision:` the [`Workspace`] is resolved from the window root at activation time
    /// (the shape `response_viewer::save_body` reaches its toast through), not held on
    /// the list: this step may not change the constructor or its callers. The root
    /// update is deferred — a window cannot be updated while it is on the update stack,
    /// which is exactly where a click or a keystroke runs.
    fn open_row(&mut self, entry: HistoryEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_entry_id = entry.id;
        cx.notify();
        let window_handle = window.window_handle();
        cx.defer(move |cx| {
            let opened = window_handle.update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    // Handled: a window whose root is not a workspace has no center pane
                    // to open anything in (the `save_body` toast's shape).
                    return;
                };
                open_entry(&entry, &workspace, window, cx);
            });
            if opened.is_err() {
                // Handled: the window closed between the keystroke and this callback;
                // there is no pane left to open the entry in.
            }
        });
    }

    /// Read the history under the current query and land it in the rows. A failure
    /// reaches the user as [`LOAD_FAILURE_MESSAGE`] above the list (AGENTS.md — errors
    /// are never only logged).
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text();
        let store = Arc::clone(&self.store);
        let workspace_id = self.workspace_id.clone();
        self.pending_load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { read_entries(&store, &workspace_id, &query) })
                .await;
            let landed = this.update(cx, |this, cx| {
                this.is_loading = false;
                match loaded {
                    Ok(entries) => {
                        this.error = None;
                        this.rows = Arc::new(group_by_day(entries));
                    }
                    Err(_error) => {
                        this.error = Some(SharedString::new_static(LOAD_FAILURE_MESSAGE));
                    }
                }
                cx.notify();
            });
            if landed.is_err() {
                // Handled: the panel went away with its window before the read landed;
                // there is nowhere left to show the result.
            }
        }));
    }

    /// One row's content — the wrapper `VirtualList` draws around it carries the row's
    /// id and test selector.
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let rows = Arc::clone(&self.rows);
        match rows.get(index) {
            Some(HistoryRow::Day { label, .. }) => self.render_day_header(label.clone(), cx),
            Some(HistoryRow::Entry(entry)) => self.render_entry(entry, cx),
            // The list is told the row count of the same snapshot it draws ids from, so
            // an out-of-range index cannot occur — and must not panic if it ever does.
            None => div().into_any_element(),
        }
    }

    fn render_day_header(&self, label: SharedString, cx: &App) -> AnyElement {
        h_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .items_center()
            .px(DynamicSpacing::Base08.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
            .into_any_element()
    }

    fn render_entry(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) -> AnyElement {
        let target_id = ElementId::NamedChild(
            Arc::new(entry_element_id(entry.id)),
            SharedString::new_static("target"),
        );
        // The clickable container is a child id of the row's stable key (the
        // `command_palette::picker`'s row shape), so a click selects and opens *this*
        // entry even if the rows re-order underneath the pointer.
        let row_id = ElementId::NamedChild(
            Arc::new(entry_element_id(entry.id)),
            SharedString::new_static("row"),
        );
        let selected = entry.id.is_some() && entry.id == self.selected_entry_id;
        let clicked_entry = entry.clone();
        v_flex()
            .id(row_id)
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .justify_center()
            .px(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base01.rems(cx))
            .rounded_sm()
            .cursor_pointer()
            .hover(|row| row.bg(cx.theme().colors().ghost_element_hover))
            .when(selected, |row| row.bg(cx.theme().colors().element_selected))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_row(clicked_entry.clone(), window, cx);
            }))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .child(
                        div().flex_none().w(px_to_rems(METHOD_COLUMN_PX)).child(
                            Label::new(entry.method.clone())
                                .size(LabelSize::XSmall)
                                .truncate()
                                .color(method_color(&entry.method, cx)),
                        ),
                    )
                    .child(
                        // The target truncates in the narrow dock and keeps its full
                        // value readable in the tooltip (the `Label::truncate` contract).
                        div()
                            .id(target_id)
                            .flex_1()
                            .min_w_0()
                            .tooltip(Tooltip::text(entry.target.clone()).builder())
                            .child(
                                Label::new(entry.target.clone())
                                    .size(LabelSize::Small)
                                    .truncate(),
                            ),
                    ),
            )
            .child(
                Label::new(entry_summary(entry))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .into_any_element()
    }
}

impl Focusable for HistoryList {
    /// The panel is one tab stop: it delegates focus to the search box, so typing works
    /// the moment the panel takes focus (the `Picker` contract does the same).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Panel for HistoryList {
    fn panel_id(&self) -> &'static str {
        PANEL_ID
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("History")
    }

    fn icon(&self) -> IconName {
        IconName::History
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Left
    }
}

impl Render for HistoryList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let error = self.error.clone();
        let search_is_empty = self.search.read(cx).text().is_empty();
        // Every element id in this panel derives from the search box's focus handle —
        // the panel's identity — so two panels can never collide in one frame (the
        // shape `command_palette::Picker` builds its ids with).
        let panel_element_id = ElementId::from(&self.search.focus_handle(cx));
        let rows = Arc::clone(&self.rows);
        let list = VirtualList::new(
            ElementId::NamedChild(
                Arc::new(panel_element_id.clone()),
                SharedString::new_static("rows"),
            ),
            rows.len(),
            move |index| row_element_id(rows.get(index)),
            cx.processor(|this, index: usize, _window, cx| this.render_row(index, cx)),
        );

        v_flex()
            .id(panel_element_id)
            .key_context(LIST_KEY_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::activate))
            .size_full()
            .font_ui(cx)
            .p(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .border_1()
                    .border_color(colors.border)
                    .rounded_sm()
                    .px(DynamicSpacing::Base06.rems(cx))
                    .child(self.search.clone()),
            )
            .map(|panel| match &error {
                Some(error) => panel.child(
                    Label::new(error.clone())
                        .size(LabelSize::Small)
                        .color(Color::Error),
                ),
                None => panel,
            })
            .child(self.body(
                list,
                search_is_empty,
                &error,
                self.focus_handle(cx).contains_focused(window, cx),
                cx,
            ))
    }
}

impl HistoryList {
    /// What the area under the search box shows: the rows when there are any, and
    /// otherwise the one state that explains why the list is empty.
    fn body(
        &self,
        list: VirtualList,
        search_is_empty: bool,
        error: &Option<SharedString>,
        list_focused: bool,
        cx: &App,
    ) -> AnyElement {
        if !self.rows.is_empty() {
            return v_flex()
                .flex_1()
                .min_h_0()
                // The list's focus style: the panel's keyboard focus lives in the search
                // box, so the rows around it say "the list is live" in the theme's focus
                // token (ux-checklist: visible focus state).
                .when(list_focused, |list_area| {
                    list_area
                        .border_1()
                        .rounded_sm()
                        .border_color(cx.theme().colors().border_focused)
                })
                .child(list)
                .into_any_element();
        }
        if error.is_some() {
            // The failure is already spelled out above the list; an empty-state caption
            // under it would contradict it.
            return div().flex_1().into_any_element();
        }
        let message = if self.is_loading {
            "Loading history…"
        } else if search_is_empty {
            "No requests sent yet"
        } else {
            "No matches"
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .into_any_element()
    }
}

/// One read of a workspace's history, off the foreground thread. The store lock guards
/// the query only — it is never held across an `.await`
/// (`.agents/rules/rust-code.md`).
fn read_entries(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
    query: &str,
) -> anyhow::Result<Vec<HistoryEntry>> {
    let store = store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store lock is poisoned"))?;
    Ok(store.history().search(workspace_id, query)?)
}

/// Fold the newest-first entries into the day-grouped rows: one [`HistoryRow::Day`]
/// header per UTC day, its entries in the order the read returned them.
fn group_by_day(entries: Vec<HistoryEntry>) -> Vec<HistoryRow> {
    let mut rows = Vec::with_capacity(entries.len());
    let mut current_day: Option<Date> = None;
    for entry in entries {
        let date = entry.created_at.date();
        if current_day != Some(date) {
            current_day = Some(date);
            rows.push(HistoryRow::Day {
                date,
                label: day_label(date),
            });
        }
        rows.push(HistoryRow::Entry(Box::new(entry)));
    }
    rows
}

/// The header text for one day's group: "Today" and "Yesterday" against the UTC day the
/// entries are grouped by (the same clock `group_by_day` buckets them with), the ISO
/// date for anything older.
fn day_label(date: Date) -> SharedString {
    let today = OffsetDateTime::now_utc().date();
    if date == today {
        SharedString::new_static("Today")
    } else if Some(date) == today.previous_day() {
        SharedString::new_static("Yesterday")
    } else {
        SharedString::from(date.to_string())
    }
}

/// One row's element id — the item's stable identity (a stored entry id, a day's date),
/// never its row position, which shifts as the list re-groups.
fn row_element_id(row: Option<&HistoryRow>) -> ElementId {
    match row {
        Some(HistoryRow::Day { date, .. }) => ElementId::from(format!("history-day-{date}")),
        Some(HistoryRow::Entry(entry)) => entry_element_id(entry.id),
        None => ElementId::from("history-row"),
    }
}

/// One entry row's element id: its stored id, which survives search re-ordering.
fn entry_element_id(id: Option<i64>) -> ElementId {
    match id {
        Some(id) => ElementId::from(format!("history-entry-{id}")),
        None => ElementId::from("history-entry"),
    }
}

/// The theme's method token for a method label (`references/design-system.md` §1: the
/// HTTP verbs are theme tokens). `Color::Custom` only resolves the token — the theme
/// still owns the value — and a custom verb, which has no token, falls back to muted
/// text.
fn method_color(method: &str, cx: &App) -> Color {
    let status = cx.theme().status();
    let token = match method {
        "GET" => status.method_get,
        "POST" => status.method_post,
        "PUT" => status.method_put,
        "PATCH" => status.method_patch,
        "DELETE" => status.method_delete,
        "HEAD" => status.method_head,
        "OPTIONS" => status.method_options,
        _ => return Color::Muted,
    };
    Color::Custom(token)
}

/// The one meta line of an entry row: status and duration (REQ-HIST-01), never the
/// request or response body (REQ-HIST-03).
fn entry_summary(entry: &HistoryEntry) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(code) = entry.status {
        parts.push(code.to_string());
    }
    if let Some(duration_ms) = entry.duration_ms {
        parts.push(format!("{duration_ms} ms"));
    }
    if parts.is_empty() {
        String::from("—")
    } else {
        parts.join(" · ")
    }
}
