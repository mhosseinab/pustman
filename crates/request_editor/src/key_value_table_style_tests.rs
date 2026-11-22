//! The key/value table's grid (#220): a boxed grid with readable column headers, a
//! "Use" first column, and a clear add-row control under it. Every assertion is on what
//! paints — the header text a table draws under its debug key and the painted bounds of
//! the grid and its controls.
//!
//! `Decision:` the grid's 1 px lines are paint, and `VisualTestContext::debug_bounds`
//! records layout bounds only (gpui `app/test_context.rs`), so the box border is pinned
//! through its layout consequence (every child sits one pixel inside it) while the row
//! and column separators are compared against the blueprint in the fix's `Decision:`
//! lines (S03/R03).

use std::collections::BTreeMap;

use api_model::KeyValue;
use gpui::{
    AppContext, Bounds, Context, Entity, IntoElement, Pixels, Render, TestAppContext,
    VisualTestContext, Window, px,
};
use ui::components::table::{
    TABLE_GRID_SELECTOR, TABLE_HEADER_SELECTOR, table_header_selector, table_row_selector,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::{ADD_ROW_SELECTOR, KeyValueTable, KeyValueTableShape};

/// Probes of known sizes: the assertions compare the table's painted text against what
/// each design-system label size paints on this host's fonts, so they hold whatever
/// font the test machine resolves.
const XSMALL_PROBE_SELECTOR: &str = "probe-label-xsmall";
const SMALL_PROBE_SELECTOR: &str = "probe-label-small";

/// One editable table with two rows, beside the probe labels.
struct TableHarness {
    table: Entity<KeyValueTable>,
}

impl TableHarness {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Editable, cx));
        table.update(cx, |table, cx| {
            table.set_rows(
                vec![row("status", "processing"), row("limit", "20")],
                window,
                cx,
            );
        });
        Self { table }
    }
}

impl Render for TableHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(self.table.clone())
            .child(probe(XSMALL_PROBE_SELECTOR, LabelSize::XSmall))
            .child(probe(SMALL_PROBE_SELECTOR, LabelSize::Small))
    }
}

/// One seeded row.
fn row(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: Some(String::from(value)),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// A label of `size` painting under `selector`.
fn probe(selector: &'static str, size: LabelSize) -> impl IntoElement {
    div()
        .debug_selector(|| String::from(selector))
        .child(Label::new(SharedString::new_static("probe")).size(size))
}

fn open_harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (_harness, cx) = cx.add_window_view(TableHarness::new);
    draw_window(cx);
    cx
}

/// Whether anything paints under `selector` (`debug_bounds` takes `&'static str`, so
/// the built key is leaked — fine for a test's short life).
fn shown(selector: String, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(leak(selector)).is_some()
}

/// The painted bounds under `selector`.
fn bounds(selector: String, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let selector = leak(selector);
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

fn leak(selector: String) -> &'static str {
    selector.leak()
}

#[gpui::test]
fn test_the_first_column_is_titled_use_not_enabled(cx: &mut TestAppContext) {
    let cx = open_harness(cx);

    assert!(
        shown(table_header_selector("Use"), cx),
        "the first column header reads 'Use'"
    );
    assert!(
        !shown(table_header_selector("Enabled"), cx),
        "no column header still reads 'Enabled'"
    );
}

#[gpui::test]
fn test_column_headers_paint_at_a_readable_size(cx: &mut TestAppContext) {
    let cx = open_harness(cx);
    let header = bounds(table_header_selector("Key"), cx);
    let small = bounds(String::from(SMALL_PROBE_SELECTOR), cx);
    let xsmall = bounds(String::from(XSMALL_PROBE_SELECTOR), cx);

    assert!(
        header.size.height >= small.size.height,
        "a column header paints at the 12 px secondary-label size, not the tiny 10 px one: \
         header {header:?}, small {small:?}"
    );
    assert!(
        header.size.height > xsmall.size.height,
        "a column header stands above a 10 px badge label: header {header:?}, xsmall {xsmall:?}"
    );
}

#[gpui::test]
fn test_add_row_is_a_full_size_control_under_the_table(cx: &mut TestAppContext) {
    let cx = open_harness(cx);
    let grid = bounds(String::from(TABLE_GRID_SELECTOR), cx);
    let add_row = bounds(String::from(ADD_ROW_SELECTOR), cx);

    assert!(
        add_row.size.width >= grid.size.width - px(2.),
        "the add-row control spans the table: grid {grid:?}, add-row {add_row:?}"
    );
    assert!(
        add_row.size.height >= px(28.),
        "the add-row control is a full-size 28 px affordance, not a dense-row button: {add_row:?}"
    );
}

#[gpui::test]
fn test_the_table_grid_is_boxed(cx: &mut TestAppContext) {
    let cx = open_harness(cx);
    let grid = bounds(String::from(TABLE_GRID_SELECTOR), cx);
    let header = bounds(String::from(TABLE_HEADER_SELECTOR), cx);
    let last_row = bounds(table_row_selector(1), cx);

    for (edge, inset) in [
        ("left", header.left() - grid.left()),
        ("top", header.top() - grid.top()),
        ("right", grid.right() - header.right()),
        ("bottom", grid.bottom() - last_row.bottom()),
    ] {
        assert_eq!(
            inset,
            px(1.),
            "the grid draws its box border — every band sits one pixel inside the {edge} edge: \
             grid {grid:?}, header {header:?}, last row {last_row:?}"
        );
    }
}
