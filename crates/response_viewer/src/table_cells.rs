//! The table cells the response's text tables share: one column heading and one
//! selectable text cell.
//!
//! The Headers and Cookies tables each carried their own copy of both (S30e review 1
//! finding 3); one implementation keeps their look, their truncation and their test
//! hooks in step, and is where the Timeline tab's and the cookie dialog's cells move to
//! (S31a review 1, S36a review 1).
//!
//! `Decision:` a heading paints under `column-heading-<heading>` and a cell under the id
//! its table passes in — the debug-selector contract [`crate::text_selection`] already
//! uses — so a test can measure either table without knowing which one it is looking at
//! (`table_cells_tests`). S05/R05: both tables show one response's fields and must read
//! alike; `docs/postman_redesign_pack/01_specs/design_system.md` fixes their type sizes.

use gpui::{ElementId, Stateful, WeakEntity};
use ui::prelude::*;

use crate::text_selection::{CellSelector, SelectableCells, selectable_cell};

/// The debug key one column heading paints under: the test hook that lets a test see the
/// headings of any response table (`table_cells_tests`).
pub(crate) fn column_heading_selector(heading: &str) -> String {
    format!("column-heading-{heading}")
}

/// One column heading above the rows: the muted label of the design system's metadata
/// size, taking an equal share of the row like the cells below it.
pub(crate) fn column_heading(heading: &'static str) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .debug_selector(move || column_heading_selector(heading))
        .child(
            Label::new(heading)
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
}

/// One table cell: selectable text in the content font, truncated with the full value
/// behind a tooltip. `index` is its place in the flattened cells the table's
/// [`CellSelector`] was built from; the table adds the id and the layout around it.
pub(crate) fn cell<V: SelectableCells>(
    id: ElementId,
    index: usize,
    text: SharedString,
    selection: &CellSelector,
    view: &WeakEntity<V>,
    cx: &App,
) -> Stateful<Div> {
    selectable_cell(id, index, text, selection.range_on(index), view.clone(), cx)
        .flex_1()
        .font_buffer(cx)
        .text_ui_sm(cx)
        .text_color(cx.theme().colors().text)
}
