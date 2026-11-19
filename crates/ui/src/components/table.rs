//! A data table: columns, a header row and rows of cells.
//!
//! The table lays things out; it does not own them. A params table passes toggle and text
//! cells, a response-headers table passes labels, and a long one is virtualized by putting
//! the rows in a [`crate::List`] instead. Column widths are `AbsoluteLength`s so a fixed
//! column (the "enabled" box) never squeezes the text column next to it.

use gpui::{
    AbsoluteLength, AnyElement, App, ElementId, InteractiveElement, IntoElement, Keystroke,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::color::Color;
use crate::components::button::ClickHandler;
use crate::components::label::{Label, LabelSize};
use crate::components::toggle::ACTIVATION_CONTEXT;
use crate::components::tooltip::Tooltip;
use crate::h_flex;
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::v_flex;

/// One column: what it is called and how wide it is.
#[derive(Clone, Debug)]
pub struct TableColumn {
    label: SharedString,
    width: Option<AbsoluteLength>,
}

impl TableColumn {
    /// A column that takes an equal share of what is left.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            width: None,
        }
    }

    /// Pin the column's width (the "enabled" checkbox column, a method badge).
    pub fn width(mut self, width: impl Into<AbsoluteLength>) -> Self {
        self.width = Some(width.into());
        self
    }
}

/// One row: a stable id, the cells in column order, and what clicking the row does.
pub struct TableRow {
    id: ElementId,
    cells: Vec<AnyElement>,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl TableRow {
    /// An empty row identified by `id`. Ids must be stable across reorders so focus and
    /// hover do not jump to the wrong row.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            cells: Vec::new(),
            disabled: false,
            on_click: None,
        }
    }

    /// Append one cell.
    pub fn cell(mut self, cell: impl IntoElement) -> Self {
        self.cells.push(cell.into_any_element());
        self
    }

    /// Append several cells, in column order.
    pub fn cells(mut self, cells: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.cells
            .extend(cells.into_iter().map(|cell| cell.into_any_element()));
        self
    }

    /// Make the whole row clickable.
    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }

    /// Grey the row out and make it inert. The row's own handler is dropped, and the row
    /// swallows clicks and Enter/Space before they reach the cells: cells are opaque
    /// [`AnyElement`]s the table cannot switch off from the outside, so a disabled row
    /// blocks their activation at its own border. Build cell controls with
    /// [`crate::Disableable::disabled`] as well, so they draw as disabled and leave the
    /// tab order (a disabled row blocks activation but not focus).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// A table of rows under a header.
#[derive(IntoElement)]
pub struct Table {
    id: ElementId,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    striped: bool,
}

impl Table {
    /// An empty table with no columns.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            columns: Vec::new(),
            rows: Vec::new(),
            striped: false,
        }
    }

    /// Add a column, left to right.
    pub fn column(mut self, column: TableColumn) -> Self {
        self.columns.push(column);
        self
    }

    /// Add a row, top to bottom.
    pub fn row(mut self, row: TableRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Give every other row a slightly raised background, so wide rows are easy to follow.
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }
}

impl RenderOnce for Table {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let columns = self.columns;
        let table_id = self.id.clone();

        let header = h_flex()
            .w_full()
            .font_ui(cx)
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .border_b_1()
            .border_color(colors.border_variant)
            .children(columns.iter().enumerate().map(|(index, column)| {
                div()
                    .id((table_id.clone(), format!("header-{index}")))
                    // The header label truncates, so its tooltip carries the full value.
                    .tooltip(Tooltip::text(column.label.clone()).builder())
                    .min_w_0()
                    .map(|this| match column.width {
                        Some(width) => this.w(width),
                        None => this.flex_1(),
                    })
                    .child(
                        Label::new(column.label.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .truncate(),
                    )
            }));

        let rows = self.rows.into_iter().enumerate().map(|(index, row)| {
            let striped = self.striped && index % 2 == 1;
            let disabled = row.disabled;
            let on_click = row.on_click;
            h_flex()
                .id(row.id)
                .w_full()
                .font_ui(cx)
                .gap(DynamicSpacing::Base04.rems(cx))
                .px(DynamicSpacing::Base04.rems(cx))
                .py(DynamicSpacing::Base02.rems(cx))
                .border_b_1()
                .border_color(colors.border_variant)
                .map(|this| {
                    if striped {
                        this.bg(colors.panel_background)
                    } else {
                        this
                    }
                })
                .map(|this| {
                    if disabled {
                        // Cells are opaque AnyElements the table cannot switch off from
                        // the outside, so a disabled row swallows activation at its own
                        // border: capture runs outer-first, and stopping propagation keeps
                        // clicks and Enter/Space from ever reaching cell content.
                        this.opacity(0.5)
                            .capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
                            .capture_any_mouse_up(|_, _, cx| cx.stop_propagation())
                            .capture_key_down(|event, _, cx| {
                                if is_activation_key(&event.keystroke) {
                                    cx.stop_propagation();
                                }
                            })
                            .capture_key_up(|event, _, cx| {
                                if is_activation_key(&event.keystroke) {
                                    cx.stop_propagation();
                                }
                            })
                    } else {
                        this.hover(|style| style.bg(colors.ghost_element_hover))
                    }
                })
                .map(|this| {
                    // A disabled row is inert (see [`TableRow::disabled`]), and a clickable
                    // row must be reachable without the mouse: it keeps a focus handle per
                    // row id, and Enter or Space pressed and released runs the same handler
                    // as a click — once per press, never on key auto-repeat.
                    if disabled {
                        this
                    } else if let Some(on_click) = on_click {
                        this.cursor_pointer()
                            .role(gpui::Role::Row)
                            .focusable()
                            .tab_stop(true)
                            .focus_visible(|style| {
                                style.border_color(cx.theme().colors().border_focused)
                            })
                            .key_context(ACTIVATION_CONTEXT)
                            .on_click(move |event, window, cx| {
                                cx.stop_propagation();
                                on_click(event, window, cx);
                            })
                    } else {
                        this
                    }
                })
                .children(
                    row.cells
                        .into_iter()
                        .zip(columns.iter())
                        .map(|(cell, column)| {
                            div()
                                .min_w_0()
                                .map(|this| match column.width {
                                    Some(width) => this.w(width),
                                    None => this.flex_1(),
                                })
                                .child(cell)
                        }),
                )
        });

        v_flex()
            .id(self.id)
            .font_ui(cx)
            .w_full()
            .child(header)
            .children(rows)
    }
}

/// Enter or Space with no modifiers — the keys GPUI turns into a click on the focused
/// control. A disabled row must not let them through to its cells.
fn is_activation_key(keystroke: &Keystroke) -> bool {
    matches!(keystroke.key.as_str(), "enter" | "space") && !keystroke.modifiers.modified()
}
