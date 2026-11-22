//! The key/value table's announcements (#140): removing a row that is not there is a
//! no-op and must not announce [`KeyValueTableEvent::RowsChanged`] — an owner hearing
//! one rebuilds the URL and the request draft off rows that did not move.
//!
//! `Decision:` the announcement counts are read through a subscriber (the owner's own
//! path, `cx.subscribe`) rather than any test hook: what a test asserts here is exactly
//! what a tab hears (S03/R03: "Validation never silently rewrites the intended
//! payload" — a phantom change event makes the owner rewrite the draft).

use std::cell::Cell;
use std::rc::Rc;

use api_model::KeyValue;
use gpui::{
    Context, Entity, IntoElement, Render, Subscription, TestAppContext, VisualTestContext, Window,
};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::{KeyValueTable, KeyValueTableEvent, KeyValueTableShape};

/// An id no row ever carries: the table's row ids count up from zero.
const MISSING_ROW_ID: u64 = 4242;

/// One editable table with a single seeded row.
struct TableHarness {
    table: Entity<KeyValueTable>,
}

impl TableHarness {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| KeyValueTable::new(KeyValueTableShape::Editable, cx));
        table.update(cx, |table, cx| {
            table.set_rows(
                vec![KeyValue {
                    key: String::from("status"),
                    value: Some(String::from("processing")),
                    description: None,
                    enabled: true,
                    extra: Default::default(),
                }],
                window,
                cx,
            );
        });
        Self { table }
    }
}

impl Render for TableHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(self.table.clone())
    }
}

/// The [`KeyValueTableEvent::RowsChanged`] announcements reaching a subscriber, counted
/// while the watch lives (a dropped subscription stops firing).
struct RowChangeWatch {
    announcements: Rc<Cell<usize>>,
    _subscription: Subscription,
}

impl RowChangeWatch {
    fn new(table: &Entity<KeyValueTable>, cx: &mut VisualTestContext) -> Self {
        let announcements = Rc::new(Cell::new(0));
        let seen = Rc::clone(&announcements);
        let _subscription = cx.update(|_window, cx| {
            cx.subscribe(table, move |_, _event: &KeyValueTableEvent, _| {
                seen.set(seen.get() + 1);
            })
        });
        Self {
            announcements,
            _subscription,
        }
    }

    /// How many change announcements arrived.
    fn count(&self) -> usize {
        self.announcements.get()
    }
}

/// The table under test, and the announcements its owner would hear.
fn open_table(
    cx: &mut TestAppContext,
) -> (
    Entity<KeyValueTable>,
    RowChangeWatch,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (harness, cx) = cx.add_window_view(TableHarness::new);
    draw_window(cx);
    let table = cx.read(|cx| harness.read(cx).table.clone());
    let watch = RowChangeWatch::new(&table, cx);
    (table, watch, cx)
}

#[gpui::test]
fn test_removing_a_row_that_is_not_there_announces_nothing(cx: &mut TestAppContext) {
    let (table, watch, cx) = open_table(cx);

    cx.update(|_window, cx| table.update(cx, |table, cx| table.remove_row(MISSING_ROW_ID, cx)));
    cx.run_until_parked();

    assert_eq!(
        watch.count(),
        0,
        "removing no row at all must announce no RowsChanged"
    );
    let rows = cx.read(|cx| table.read(cx).rows());
    assert_eq!(rows.len(), 1, "the row that was not removed is still there");
}

#[gpui::test]
fn test_removing_a_row_announces_the_change_once(cx: &mut TestAppContext) {
    let (table, watch, cx) = open_table(cx);
    let row_id = cx.read(|cx| table.read(cx).row_ids().first().copied());
    let Some(row_id) = row_id else {
        panic!("the harness seeds one row");
    };

    cx.update(|_window, cx| table.update(cx, |table, cx| table.remove_row(row_id, cx)));
    cx.run_until_parked();

    assert_eq!(
        watch.count(),
        1,
        "a row that goes away is announced exactly once"
    );
    let rows = cx.read(|cx| table.read(cx).rows());
    assert!(rows.is_empty(), "the removed row is gone: {rows:?}");
}
