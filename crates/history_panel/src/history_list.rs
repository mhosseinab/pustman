//! The History panel's list (PRD §8.5, REQ-HIST-01/04): the recorded sends of one
//! workspace, grouped by day and filterable through a search box.
//!
//! `storage` is synchronous (ADR-0002), so every read runs on `cx.background_executor`
//! and the foreground never touches SQLite. The read is kicked off by the constructor
//! and re-run on every search edit; the [`Task`] is stored, so a new query cancels the
//! read it supersedes and only the newest result can land. Rows carry method, target,
//! status and time (REQ-HIST-01) — never request or response content (REQ-HIST-03).

use std::sync::{Arc, Mutex};

use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    Window, div,
};
use storage::{HistoryEntry, LocalStore};
use text_input::{TextInput, TextInputEvent};
use time::{Date, OffsetDateTime};
use ui::prelude::*;
use workspace::{DockPosition, Panel};

#[cfg(test)]
#[path = "history_list_tests.rs"]
mod tests;

/// The panel's stable id: its dock selector entry, its hitbox and the id
/// [`crate::focus_history_panel`] activates it under.
pub const PANEL_ID: &str = "history";

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
    /// the new query.
    fn on_search_event(
        &mut self,
        _search: &Entity<TextInput>,
        event: &TextInputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let TextInputEvent::Changed = event {
            self.reload(cx);
        }
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
    fn render_row(&self, index: usize, cx: &App) -> AnyElement {
        match self.rows.get(index) {
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
            .px(DynamicSpacing::Base04.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
            .into_any_element()
    }

    fn render_entry(&self, entry: &HistoryEntry, cx: &App) -> AnyElement {
        let target_id = ElementId::NamedChild(
            Arc::new(entry_element_id(entry.id)),
            SharedString::new_static("target"),
        );
        v_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .justify_center()
            .px(DynamicSpacing::Base04.rems(cx))
            .gap(DynamicSpacing::Base01.rems(cx))
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .size_full()
            .font_ui(cx)
            .p(DynamicSpacing::Base04.rems(cx))
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .border_1()
                    .border_color(colors.border)
                    .rounded_sm()
                    .px(DynamicSpacing::Base04.rems(cx))
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
            .child(self.body(list, search_is_empty, &error))
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
    ) -> AnyElement {
        if !self.rows.is_empty() {
            return v_flex().flex_1().min_h_0().child(list).into_any_element();
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
