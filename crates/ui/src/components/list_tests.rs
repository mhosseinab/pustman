//! The virtual list draws only the rows the window can show: 10_000 rows in a window of a
//! few rows' height must cost a handful of row renders, and the rows far outside the
//! viewport must never be laid out at all.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Context, ElementId, IntoElement, ParentElement, Render, ScrollStrategy, Styled, TestAppContext,
    UniformListScrollHandle, Window, div,
};

use crate::components::label::Label;
use crate::components::list::VirtualList;
use crate::test_support::{draw_window, init_test};

/// How many rows the test's list holds — far more than any window can show.
const ITEM_COUNT: usize = 10_000;

/// A window of one list over [`ITEM_COUNT`] numbered rows, counting every row the list
/// asks for. The rows are their own stable ids (`row-0` … `row-{ITEM_COUNT - 1}`), so the
/// counts and the selectors below track items, not viewport positions.
struct VirtualListHarness {
    rendered_rows: Rc<Cell<usize>>,
    scroll_handle: UniformListScrollHandle,
}

impl Render for VirtualListHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rendered_rows = self.rendered_rows.clone();
        div().size_full().child(
            VirtualList::new(
                "virtual-list",
                ITEM_COUNT,
                |index| ElementId::from(format!("row-{index}")),
                move |index, _window, _cx| {
                    rendered_rows.set(rendered_rows.get() + 1);
                    Label::new(format!("Row {index}")).into_any_element()
                },
            )
            .track_scroll(self.scroll_handle.clone()),
        )
    }
}

#[gpui::test]
fn virtual_list_renders_only_visible_rows(cx: &mut TestAppContext) {
    cx.update(init_test);
    let rendered_rows = Rc::new(Cell::new(0));
    let scroll_handle = UniformListScrollHandle::new();
    let (_list, cx) = cx.add_window_view(|_, _cx| VirtualListHarness {
        rendered_rows: rendered_rows.clone(),
        scroll_handle: scroll_handle.clone(),
    });
    draw_window(cx);

    let rendered = rendered_rows.get();
    assert!(
        (1..500).contains(&rendered),
        "the list rendered {rendered} of {ITEM_COUNT} rows; only the visible ones may render"
    );
    assert!(
        cx.debug_bounds("row-0").is_some(),
        "the first row is visible and rendered"
    );
    assert!(
        cx.debug_bounds("row-5000").is_none(),
        "a row 5000 down is far outside the viewport and must not render at all"
    );
    assert!(
        cx.debug_bounds("row-9999").is_none(),
        "the last row is far outside the viewport and must not render at all"
    );

    // Scrolling to an item must move the window of rendered rows with it: the second
    // frame renders item 5000's neighbourhood, and nothing of row 0's.
    scroll_handle.scroll_to_item(5000, ScrollStrategy::Top);
    rendered_rows.set(0);
    draw_window(cx);

    let rendered = rendered_rows.get();
    assert!(
        (1..500).contains(&rendered),
        "the scrolled frame rendered {rendered} rows; only the visible ones may render"
    );
    assert!(
        cx.debug_bounds("row-5000").is_some(),
        "the scrolled-to row is at the top of the viewport and rendered"
    );
    assert!(
        cx.debug_bounds("row-0").is_none(),
        "the first row scrolled out of the viewport and must not render at all"
    );
}
