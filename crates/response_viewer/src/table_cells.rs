//! The table cells the response's text tables share: one column heading and one
//! selectable text cell, each in the width its column declares.
//!
//! The Headers and Cookies tables each carried their own copy of both (S30e review 1
//! finding 3); one implementation keeps their look, their truncation and their test
//! hooks in step. The Timeline tab's cells are the same cell in the shape its table
//! needs — laid out around itself rather than sized by a [`ColumnWidth`]
//! ([`selectable_text_cell`], S31a review 1).
//!
//! `Decision:` a heading paints under `column-heading-<heading>` and a cell under the id
//! its table passes in — the debug-selector contract [`crate::text_selection`] already
//! uses — so a test can measure either table without knowing which one it is looking at
//! (`table_cells_tests`). S05/R05: both tables show one response's fields and must read
//! alike; `docs/postman_redesign_pack/01_specs/design_system.md` fixes their type sizes.

use gpui::{ElementId, Stateful, WeakEntity};
use ui::prelude::*;

use crate::text_selection::{CellSelector, SelectableCells, selectable_cell};

/// How wide one column of a table is: a share of the row's remaining width, or a
/// pinned width in rems for short content that must not take room from what the user
/// reads.
#[derive(Clone, Copy)]
pub(crate) enum ColumnWidth {
    /// `weight` shares of what the row has left after the fixed columns: `1.0` is one
    /// equal share, `3.0` is three of them.
    Share(f32),
    /// A pinned width, in rems so it scales with the UI font size.
    Fixed(Rems),
}

impl ColumnWidth {
    /// Lay `element` out as this column.
    pub(crate) fn sized<E: Styled>(self, element: E) -> E {
        match self {
            Self::Share(weight) => element.flex_1().flex_grow(weight).min_w_0(),
            Self::Fixed(width) => element.flex_none().w(width).min_w_0(),
        }
    }
}

/// The debug key one column heading paints under: the test hook that lets a test see the
/// headings of any response table (`table_cells_tests`).
pub(crate) fn column_heading_selector(heading: &str) -> String {
    format!("column-heading-{heading}")
}

/// One column heading above the rows: the muted label of the design system's metadata
/// size, in a column of its table's own width.
pub(crate) fn column_heading(heading: &'static str, width: ColumnWidth) -> impl IntoElement {
    width
        .sized(div())
        .debug_selector(move || column_heading_selector(heading))
        .child(
            Label::new(heading)
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
}

/// The text every response table cell shares: the content font at the tables' text size,
/// in the theme's text colour (`01_specs/design_system.md`). A cell that mutes itself or
/// one value says so after this.
pub(crate) fn cell_text<E: StyledTypography>(element: E, cx: &App) -> E {
    element
        .font_buffer(cx)
        .text_ui_sm(cx)
        .text_color(cx.theme().colors().text)
}

/// One table cell: selectable text in the content font, truncated with the full value
/// behind a tooltip, in a column of its table's own width. `index` is its place in the
/// flattened cells the table's [`CellSelector`] was built from; the table adds the id and
/// the layout around it.
pub(crate) fn cell<V: SelectableCells>(
    id: ElementId,
    index: usize,
    text: SharedString,
    width: ColumnWidth,
    selection: &CellSelector,
    view: &WeakEntity<V>,
    cx: &App,
) -> Stateful<Div> {
    width.sized(selectable_text_cell(id, index, text, selection, view, cx))
}

/// [`cell`] before its table sizes it: the selectable cell of a table — the Timeline
/// tab's — that lays its columns out around the cell instead of asking the cell for a
/// [`ColumnWidth`].
pub(crate) fn selectable_text_cell<V: SelectableCells>(
    id: ElementId,
    index: usize,
    text: SharedString,
    selection: &CellSelector,
    view: &WeakEntity<V>,
    cx: &App,
) -> Stateful<Div> {
    cell_text(
        selectable_cell(id, index, text, selection.range_on(index), view.clone(), cx),
        cx,
    )
}
