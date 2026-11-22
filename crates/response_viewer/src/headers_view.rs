//! The response headers table: every field, duplicates kept, in wire order
//! (REQ-RESP-05), and a right-click menu that copies one header (REQ-RESP-07).
//!
//! The rows are a snapshot taken when the response arrives ([`HeadersView::set_headers`]);
//! render lays them out through [`VirtualList`], so a response with hundreds of header
//! fields only shapes the rows the viewport can show.
//!
//! `Decision:` the row menu opens where the user asked for it — the pointer on
//! right-click, the row's corner on the keyboard — through `gpui::Anchored`, which keeps
//! it inside the window. It is keyboard-reachable: every row is a tab stop and Enter or
//! Space on the focused row opens its menu (GPUI's synthesized click, the same one the
//! [`ui::components::behavior::Clickable`] controls answer), and the menu itself is one
//! tab stop with Up/Down/Enter/Escape (`ui::ContextMenu`). The physical context-menu key
//! is not bound: no `pustman_actions` action opens a row menu and the keymaps belong to
//! later steps.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    ClipboardItem, Entity, MouseButton, MouseDownEvent, Point, Subscription, WeakEntity, anchored,
    deferred,
};
use http_engine::Header;
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

use crate::child_id;
use crate::table_cells::{cell, column_heading};
use crate::text_selection::{CellSelector, SelectableCells, TEXT_CONTEXT, selection_handlers};

/// One header field as the table shows it: name and value, as they arrived.
#[derive(Clone, Debug)]
pub struct HeaderRow {
    /// The field name, as it went on the wire.
    pub name: SharedString,
    /// The field value.
    pub value: SharedString,
    /// Which occurrence of `name` this row is. A duplicate `Set-Cookie` keeps its own row
    /// and its own element identity.
    occurrence: usize,
}

impl HeaderRow {
    /// This row's stable name: the field name plus which occurrence of it it is — the
    /// identity [`Self::key`] builds an element id from, and the row the open menu names.
    fn key_name(&self) -> SharedString {
        SharedString::from(format!("{}#{}", self.name, self.occurrence))
    }

    /// This row's element identity under `base`: its name and which occurrence of that
    /// name it is — stable across scrolling and independent of the row's position, so
    /// focus and hover never jump to a different field.
    fn key(&self, base: &ElementId) -> ElementId {
        child_id(base, self.key_name())
    }

    /// What the row's "Copy" menu puts on the clipboard: `Name: value`.
    fn copy_text(&self) -> String {
        format!("{}: {}", self.name, self.value)
    }
}

/// The open "Copy" menu of one header row: the menu itself, the row it copies and where
/// it opened (window coordinates).
#[derive(Clone)]
struct RowMenu {
    menu: Entity<ContextMenu>,
    row_key: SharedString,
    /// The row's focus handle, so the menu can hand focus back when it dismisses.
    row_focus: FocusHandle,
    position: Point<Pixels>,
}

/// The headers table: the rows of one response, and the two-column layout that draws
/// them.
pub struct HeadersView {
    rows: Arc<Vec<HeaderRow>>,
    /// One focus handle per row, built with the rows — a handle made in render would be
    /// new every frame and drop the row's focus with it.
    row_focus: Arc<Vec<FocusHandle>>,
    /// The row menu while it is open.
    row_menu: Option<RowMenu>,
    row_menu_subscription: Option<Subscription>,
    /// The hand-made selection over the names and values, flattened row by row.
    selection: CellSelector,
    /// Where focus lands when a press selects outside any row, so copy has a target.
    focus_handle: FocusHandle,
}

impl SelectableCells for HeadersView {
    fn cell_selector(&mut self) -> &mut CellSelector {
        &mut self.selection
    }
}

impl HeadersView {
    /// An empty table, waiting for a response.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            rows: Arc::new(Vec::new()),
            row_focus: Arc::new(Vec::new()),
            row_menu: None,
            row_menu_subscription: None,
            selection: CellSelector::default(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Show these header fields exactly as they arrived: every occurrence, in order. The
    /// rows are rebuilt here (never in `render`), so render stays a pure layout of what
    /// this snapshot holds.
    pub fn set_headers(&mut self, headers: &[Header], cx: &mut Context<Self>) {
        let mut occurrences: HashMap<String, usize> = HashMap::new();
        let rows = headers
            .iter()
            .map(|header| {
                let counter = occurrences.entry(header.name.clone()).or_default();
                let occurrence = *counter;
                *counter += 1;
                HeaderRow {
                    name: header.name.as_str().into(),
                    value: header.value.as_str().into(),
                    occurrence,
                }
            })
            .collect();
        self.rows = Arc::new(rows);
        // `name: value`, one row per line — the shape the row menu's Copy and HTTP use.
        self.selection.set(
            self.rows
                .iter()
                .flat_map(|row| [(row.name.clone(), ": "), (row.value.clone(), "\n")])
                .collect(),
        );
        self.row_focus = Arc::new(
            self.rows
                .iter()
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
        );
        // A menu open on the previous snapshot would name a row that is gone.
        self.row_menu = None;
        self.row_menu_subscription = None;
        cx.notify();
    }

    /// The rows the table draws, top to bottom.
    pub fn rows(&self) -> &[HeaderRow] {
        &self.rows
    }

    /// Open the "Copy" menu for one row at `position` (window coordinates). Opening one
    /// while another is open replaces it: the new click names the row the user means.
    fn open_row_menu(
        &mut self,
        row_key: SharedString,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((row_index, row)) = self
            .rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.key_name() == row_key)
            .map(|(index, row)| (index, row.clone()))
        else {
            return;
        };
        let Some(row_focus) = self.row_focus.get(row_index).cloned() else {
            return;
        };
        let copy_text = row.copy_text();
        let menu = cx.new(|cx| {
            ContextMenu::new(cx).entry(
                SharedString::new_static("Copy"),
                move |_event, _window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                },
            )
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.row_menu = Some(RowMenu {
            menu,
            row_key,
            row_focus,
            position,
        });
        self.row_menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The menu is done — its entry ran or Escape backed out — so it goes away and focus
    /// returns to the row it was opened from (the shape `request_editor::url_bar` closes
    /// its dropdown with).
    fn on_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        if let Some(open) = self.row_menu.take() {
            self.row_menu_subscription = None;
            window.focus(&open.row_focus, cx);
            cx.notify();
        }
    }
}

impl Render for HeadersView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Arc::clone(&self.rows);
        let row_focus = Arc::clone(&self.row_focus);
        let open_menu = self.row_menu.clone();
        let view = cx.weak_entity();
        let selection = Arc::new(self.selection.clone());
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("headers-view", cx.entity_id()));
        let root = v_flex()
            .size_full()
            .key_context(TEXT_CONTEXT)
            .track_focus(&self.focus_handle);
        selection_handlers(root, cx)
            .child(
                h_flex()
                    .w_full()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base02.rems(cx))
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(column_heading("Name"))
                    .child(column_heading("Value")),
            )
            .map(|this| {
                if rows.is_empty() {
                    this.child(empty_state())
                } else {
                    let row_keys = Arc::clone(&rows);
                    let key_base = base.clone();
                    let row_count = rows.len();
                    let table = RowTable {
                        rows,
                        base: base.clone(),
                        row_focus,
                        open_menu,
                        view,
                        selection,
                    };
                    this.child(VirtualList::new(
                        child_id(&base, "rows"),
                        row_count,
                        move |index| match row_keys.get(index) {
                            Some(row) => row.key(&key_base),
                            None => child_id(&key_base, format!("row-{index}")),
                        },
                        move |index, _window, cx| render_row(&table, index, cx),
                    ))
                }
            })
    }
}

/// What every row of one frame renders from, moved into the list's row closure.
struct RowTable {
    rows: Arc<Vec<HeaderRow>>,
    base: ElementId,
    row_focus: Arc<Vec<FocusHandle>>,
    open_menu: Option<RowMenu>,
    view: WeakEntity<HeadersView>,
    selection: Arc<CellSelector>,
}

/// One header row: name and value, each truncating with its full value in a tooltip. The
/// row is a tab stop; right-click or Enter/Space on it opens its "Copy" menu, and while
/// the menu is open it floats over the table at where the row opened it.
fn render_row(table: &RowTable, index: usize, cx: &mut App) -> AnyElement {
    let RowTable {
        rows,
        base,
        row_focus,
        open_menu,
        view,
        selection,
    } = table;
    let (Some(row), Some(focus_handle)) = (rows.get(index), row_focus.get(index)) else {
        return div().into_any_element();
    };
    let row_id = row.key(base);
    let menu_overlay = open_menu
        .as_ref()
        .filter(|open| open.row_key == row.key_name())
        .map(|open| {
            let menu = open.menu.clone();
            let position = open.position;
            div()
                .absolute()
                .top_0()
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .position(position)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(menu),
                    )
                    .with_priority(1),
                )
                .into_any_element()
        });

    // Right-click opens the menu at the pointer; the keyboard's synthesized click
    // (`ClickEvent::Keyboard` — Enter or Space on the focused row) opens it at the row's
    // corner. A mouse left-click opens nothing: this menu is the row's secondary action.
    let right_click_view = view.clone();
    let right_click_key = row.key_name();
    let on_right_click = move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
        open_row_menu(
            &right_click_view,
            &right_click_key,
            event.position,
            window,
            cx,
        );
    };
    let activate_view = view.clone();
    let activate_key = row.key_name();
    let focus_back = focus_handle.clone();
    let on_activate = move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
        match event {
            ClickEvent::Keyboard(_) => {
                open_row_menu(&activate_view, &activate_key, event.position(), window, cx);
            }
            // Clicking a row puts focus on it, so the menu is one key away afterwards.
            ClickEvent::Mouse(_) | ClickEvent::Touch(_) => window.focus(&focus_back, cx),
        }
    };

    h_flex()
        .id(row_id.clone())
        .w_full()
        .relative()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .track_focus(focus_handle)
        .focus_visible(|style| style.bg(cx.theme().colors().element_selected))
        .on_mouse_down(MouseButton::Right, on_right_click)
        .on_click(on_activate)
        .child(cell(
            child_id(&row_id, "name"),
            2 * index,
            row.name.clone(),
            selection,
            view,
            cx,
        ))
        .child(cell(
            child_id(&row_id, "value"),
            2 * index + 1,
            row.value.clone(),
            selection,
            view,
            cx,
        ))
        .children(menu_overlay)
        .into_any_element()
}

/// Open the row menu for `row_key` at `position`, unless the table is already gone.
fn open_row_menu(
    view: &WeakEntity<HeadersView>,
    row_key: &SharedString,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if view
        .update(cx, |table, cx| {
            table.open_row_menu(row_key.clone(), position, window, cx)
        })
        .is_err()
    {
        // The table went away between the click and here; there is no row left to open
        // a menu for.
    }
}

/// The state where there is nothing to list: before the first response, or a response
/// that carried no header fields at all.
fn empty_state() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .child(Label::new("This response has no headers.").color(Color::Muted))
}

#[cfg(test)]
#[path = "headers_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "headers_selection_tests.rs"]
mod selection_tests;
