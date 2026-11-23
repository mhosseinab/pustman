//! The History panel's list (PRD §8.5, REQ-HIST-01/04): the recorded sends of one
//! workspace, grouped by day and filterable through a search box. Activating a row — a
//! click on it, or Enter on the keyboard selection — opens the entry through
//! [`crate::open_entry`] (REQ-HIST-02).
//!
//! `storage` is synchronous (ADR-0002), so every read runs on `cx.background_executor`
//! and the foreground never touches SQLite. The read is kicked off by the constructor,
//! re-run on every search edit, and re-run when a send is recorded — the Console feed
//! (`console::ConsoleStore`, which the send's owner pushes one entry per send into) is
//! the app's existing per-send notification, so the list refreshes live without a
//! polling timer (S33o). The [`Task`] is stored, so a new query cancels the read it
//! supersedes and only the newest result can land. Rows carry method, target,
//! status and time (REQ-HIST-01) — never request or response content (REQ-HIST-03).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use console::ConsoleStore;
use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, Focusable, Global,
    InteractiveElement, InteractiveText, IntoElement, ParentElement, Render, Role, ScrollStrategy,
    SharedString, StatefulInteractiveElement, Styled, StyledText, Subscription, Task,
    UniformListScrollHandle, Window, div,
};
use pustman_actions::workspace::NewTab;
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
#[path = "history_summary_tests.rs"]
mod summary_tests;

#[cfg(test)]
#[path = "history_row_tests.rs"]
mod row_tests;

#[cfg(test)]
#[path = "history_day_label_tests.rs"]
mod day_label_tests;

#[cfg(test)]
#[path = "history_list_activation_tests.rs"]
mod activation_tests;

#[cfg(test)]
#[path = "history_list_empty_state_tests.rs"]
mod empty_state_tests;

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

/// The New Request call to action's stable id and test selector.
const NEW_REQUEST_ID: &str = "history-new-request";

/// The Clear search call to action's stable id and test selector.
const CLEAR_SEARCH_ID: &str = "history-clear-search";

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
    /// The wake that re-derives the day labels at each UTC midnight ([
    /// [`Self::arm_day_label_wake`]). Stored so dropping the panel ends the loop.
    day_label_wake: Option<Task<()>>,
    /// The entry the keyboard is on — its stored id, which survives search re-ordering
    /// — or [`None`] while the arrows and clicks have not put it anywhere. Rows the
    /// store left without an id cannot carry it (none reach the list from `storage`).
    selected_entry_id: Option<i64>,
    /// The list's scroll state, so moving the selection scrolls its row into view.
    scroll_handle: UniformListScrollHandle,
    // Kept so the search box's edits and the per-send Console feed keep reaching us;
    // dropping them would silently stop filtering and live refresh.
    _subscriptions: Vec<Subscription>,
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
        // Live refresh (S33o): every recorded send lands one Console entry after its
        // history row is written (`request_editor::send` records the outcome only once
        // the client's background work has stored it), so the feed's notification is
        // the moment the DB holds the row — re-read then, never on a timer.
        let _recorded_observer =
            cx.observe_global::<ConsoleStore>(|this: &mut Self, cx| this.reload(cx));
        // Day labels (#110): "Today"/"Yesterday" are derived against the UTC day, so a
        // read must not freeze them. Re-derive whenever the window is activated — the
        // moment a machine that slept across a UTC midnight comes back.
        let _activation_observer =
            cx.observe_window_activation(window, |this: &mut Self, _window, cx| {
                this.refresh_day_labels(OffsetDateTime::now_utc().date(), cx);
            });
        let mut this = Self {
            store,
            workspace_id: workspace_id.into(),
            search,
            rows: Arc::new(Vec::new()),
            is_loading: true,
            error: None,
            pending_load: None,
            day_label_wake: None,
            selected_entry_id: None,
            scroll_handle: UniformListScrollHandle::new(),
            _subscriptions: vec![
                _search_subscription,
                _recorded_observer,
                _activation_observer,
            ],
        };
        this.reload(cx);
        this.arm_day_label_wake(cx);
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
                        this.rows =
                            Arc::new(group_by_day(entries, OffsetDateTime::now_utc().date()));
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

    /// Re-derive every day header's label against `today` (#110): the labels are
    /// display data — derived against the day the list is drawn on, never frozen by
    /// the read that grouped the rows — so a panel left open across UTC midnight gets
    /// its "Today" corrected by [`Self::arm_day_label_wake`] and by a window
    /// activation. Notifies only when a label actually changed.
    fn refresh_day_labels(&mut self, today: Date, cx: &mut Context<Self>) {
        let stale = self.rows.iter().any(|row| match row {
            HistoryRow::Day { date, label } => *label != day_label(*date, today),
            HistoryRow::Entry(_) => false,
        });
        if !stale {
            return;
        }
        for row in Arc::make_mut(&mut self.rows) {
            if let HistoryRow::Day { date, label } = row {
                *label = day_label(*date, today);
            }
        }
        cx.notify();
    }

    /// The wake that keeps the day labels fresh (#110): sleep until the next UTC
    /// midnight, re-derive the labels against the day that began, sleep again. The task
    /// is stored on the panel, so dropping it drops the task and the loop ends
    /// (AGENTS.md — a dropped `Task` cancels its work).
    fn arm_day_label_wake(&mut self, cx: &mut Context<Self>) {
        self.day_label_wake = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(until_next_utc_midnight(OffsetDateTime::now_utc()))
                    .await;
                let today = OffsetDateTime::now_utc().date();
                let refreshed = this.update(cx, |this, cx| this.refresh_day_labels(today, cx));
                if refreshed.is_err() {
                    // Handled: the panel went away with its window; there are no rows
                    // left to relabel and the wake dies with it.
                    break;
                }
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
                    .child(target_element(entry, cx)),
            )
            .child(meta_element(entry))
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
    /// otherwise the one state that explains why the list is empty — the design
    /// system's icon + title + call to action block, never a caption below its scale
    /// (#234).
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
        let empty_state = if self.is_loading {
            EmptyState::new(
                "history-empty-state",
                IconName::LoaderCircle,
                "Loading history…",
            )
        } else if search_is_empty {
            EmptyState::new(
                "history-empty-state",
                IconName::History,
                "No requests sent yet",
            )
            .description("Requests you send are recorded here.")
            .child(new_request_action(cx))
        } else {
            EmptyState::new("history-empty-state", IconName::Search, "No matches")
                .description("Nothing matches the current search.")
                .child(clear_search_action(&self.search))
        };
        div()
            .flex_1()
            .min_h_0()
            .child(empty_state)
            .into_any_element()
    }
}

/// The empty history's call to action: open a request and send it. It dispatches
/// [`NewTab`] like the Welcome page's button does — in the full app the workspace hands
/// that to the request editor (REQ-UI-02/03).
fn new_request_action(cx: &App) -> impl IntoElement {
    h_flex()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(
            div().debug_selector(|| NEW_REQUEST_ID.to_owned()).child(
                Button::new(NEW_REQUEST_ID, "New Request")
                    .style(ButtonStyle::Filled)
                    .start_icon(Icon::new(IconName::Plus))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTab), cx)),
            ),
        )
        .child(KeyBinding::for_action(&NewTab).size(TextSize::Default))
}

/// The no-matches call to action: drop the query and come back to the full list. The
/// clearing goes through [`TextInput::set_text`], which emits the same
/// [`TextInputEvent::Changed`] a keystroke would, so the panel re-reads exactly as it
/// does for typing.
fn clear_search_action(search: &Entity<TextInput>) -> impl IntoElement {
    let search = search.clone();
    div().debug_selector(|| CLEAR_SEARCH_ID.to_owned()).child(
        Button::new(CLEAR_SEARCH_ID, "Clear search")
            .style(ButtonStyle::Outlined)
            .on_click(move |_, _window, cx| {
                search.update(cx, |input, cx| input.set_text("", cx));
            }),
    )
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
/// header per UTC day, its entries in the order the read returned them. The headers'
/// labels are derived against `today`, the day the read ran on — [`HistoryList::refresh_day_labels`]
/// re-derives them when that day is no longer the current one (#110).
fn group_by_day(entries: Vec<HistoryEntry>, today: Date) -> Vec<HistoryRow> {
    let mut rows = Vec::with_capacity(entries.len());
    let mut current_day: Option<Date> = None;
    for entry in entries {
        let date = entry.created_at.date();
        if current_day != Some(date) {
            current_day = Some(date);
            rows.push(HistoryRow::Day {
                date,
                label: day_label(date, today),
            });
        }
        rows.push(HistoryRow::Entry(Box::new(entry)));
    }
    rows
}

/// The header text for one day's group: "Today" and "Yesterday" against the UTC day
/// `today` (the same clock `group_by_day` buckets entries with), the ISO date for
/// anything older. `today` is passed in, never read here, so the caller decides which
/// day the labels describe (#110).
fn day_label(date: Date, today: Date) -> SharedString {
    if date == today {
        SharedString::new_static("Today")
    } else if Some(date) == today.previous_day() {
        SharedString::new_static("Yesterday")
    } else {
        SharedString::from(date.to_string())
    }
}

/// How long from `now` until the next UTC midnight — the moment a "Today" an earlier
/// read derived turns wrong, so the day-label wake sleeps exactly this long (#110).
fn until_next_utc_midnight(now: OffsetDateTime) -> Duration {
    let since_midnight = Duration::from_secs(
        u64::from(now.hour()) * 60 * 60 + u64::from(now.minute()) * 60 + u64::from(now.second()),
    ) + Duration::from_nanos(u64::from(now.nanosecond()));
    Duration::from_secs(24 * 60 * 60).saturating_sub(since_midnight)
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

/// One entry row's target box test selector: the row's stable key plus `target`, the
/// `VirtualList` row-selector shape.
fn target_debug_selector(id: Option<i64>) -> String {
    format!("{}-target", entry_element_id(id))
}

/// One row's target: the URL truncates in the narrow dock, and its hover tooltip
/// carries the full value when — and only when — the drawn text is not the whole
/// target (#235): a balloon repeating a visible URL covers the row it repeats (the
/// `response_viewer` body lines' truncation-tooltip shape).
fn target_element(entry: &HistoryEntry, cx: &App) -> AnyElement {
    let selector = target_debug_selector(entry.id);
    let text_id = format!("{selector}-text");
    let text = StyledText::new(entry.target.clone());
    // The layout the text fills once the row lays it out — read at hover time to see
    // what was actually drawn.
    let drawn = text.layout().clone();
    let full_target = entry.target.clone();
    div()
        .id(ElementId::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_1()
        .min_w_0()
        .font_ui(cx)
        .text_size(TextSize::Small.rems(cx))
        .truncate()
        .child(
            InteractiveText::new(ElementId::from(text_id), text).tooltip(
                move |_index, _window, cx| {
                    // A truncation tooltip only (#235): when the drawn text is the target
                    // we hold, the hover has nothing to add.
                    if drawn.text() == full_target {
                        None
                    } else {
                        Some(Tooltip::text(full_target.clone()).into_any_view(cx))
                    }
                },
            ),
        )
        .into_any_element()
}

/// How a send ended, for rows that carry no status code to say it themselves: the two
/// markers `http_engine` records in `response_summary_json` for sends that never got a
/// response.
#[derive(Clone, Copy, Debug, PartialEq)]
enum EntryState {
    /// The send failed before a response — the summary names the error's kind.
    Failed,
    /// The send was cancelled before it finished.
    Cancelled,
}

/// The state a send ended in, from the marker `http_engine` stored with it — [`None`]
/// for a response, which names its outcome as a status code instead.
fn entry_state(entry: &HistoryEntry) -> Option<EntryState> {
    let summary = entry.response_summary_json.as_ref()?;
    if summary
        .get("cancelled")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        Some(EntryState::Cancelled)
    } else if summary.get("error").is_some_and(|error| error.is_string()) {
        Some(EntryState::Failed)
    } else {
        None
    }
}

/// One row's meta line (#235): what the send ended in, plus how long it took. A
/// response names itself as "200 · 184 ms"; a send that got no response has no status
/// code to say so, and its state is named in a pill — [`Badge`], the design system's
/// "status" tag — in the severity it deserves (`docs/postman_redesign_pack/`,
/// `01_specs/design_system.md` Response: "HTTP response versus transport failure";
/// S13/R13).
fn meta_element(entry: &HistoryEntry) -> AnyElement {
    let summary = entry_summary(entry);
    match entry_state(entry) {
        Some(EntryState::Failed) => Badge::new(summary)
            .severity(Severity::Error)
            .into_any_element(),
        Some(EntryState::Cancelled) => Badge::new(summary).into_any_element(),
        None => Label::new(summary)
            .size(LabelSize::XSmall)
            .color(Color::Muted)
            .into_any_element(),
    }
}

/// The one meta line of an entry row: outcome and duration (REQ-HIST-01), never the
/// request or response body (REQ-HIST-03). A send that got no response has no status
/// code, so its state is named instead (#235) — the bare "—" is left only for a row
/// that stores no outcome at all, which `http_engine` never records. The duration shows
/// in the shared `ui` form the Console rows and the response status line show one in
/// (#134).
fn entry_summary(entry: &HistoryEntry) -> String {
    let mut parts: Vec<String> = Vec::new();
    match entry_state(entry) {
        Some(EntryState::Failed) => parts.push(String::from("Failed")),
        Some(EntryState::Cancelled) => parts.push(String::from("Cancelled")),
        None => {
            if let Some(code) = entry.status {
                parts.push(code.to_string());
            }
        }
    }
    if let Some(duration) = entry
        .duration_ms
        .and_then(|millis| u64::try_from(millis).ok())
        .map(Duration::from_millis)
    {
        parts.push(ui::format_duration(duration));
    }
    if parts.is_empty() {
        String::from("—")
    } else {
        parts.join(" · ")
    }
}
