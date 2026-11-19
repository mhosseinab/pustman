//! The virtualized list: a window onto rows that may number in the thousands.
//!
//! [`VirtualList`] draws through GPUI's [`uniform_list`], which lays out only the rows
//! the viewport can show, and identifies every row by the caller's item key — the row's
//! element id and its test selector both come from `row_key`, never from the row's
//! position — so scroll, hover and focus state survive scrolling and reordering
//! (`references/performance.md`, `references/gpui-fundamentals.md`).

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Styled,
    UniformListScrollHandle, Window, div, uniform_list,
};

/// What a [`VirtualList`] asks for every row it lays out: the stable id of the item the
/// row shows.
type RowKey = Rc<dyn Fn(usize) -> ElementId>;

/// How a [`VirtualList`] draws one row.
type RowRenderer = Rc<dyn Fn(usize, &mut Window, &mut App) -> AnyElement>;

/// A list of same-height rows that renders only what its bounds can show.
///
/// The API is three things on purpose: how many rows there are, what the stable key of
/// each row's item is, and how to draw one row. The list owns no data, so the caller
/// keeps addressing rows by index while the *identity* of a row stays its item key.
/// Give it a bounded height — its parent's — or there is nothing to virtualize against;
/// [`RenderOnce`] fills what it is given (`size_full`).
#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    item_count: usize,
    row_key: RowKey,
    render_row: RowRenderer,
    scroll_handle: Option<UniformListScrollHandle>,
}

impl VirtualList {
    /// A list of `item_count` rows. `row_key` maps a row to the stable id of the item it
    /// shows (a request id, a header name); `render_row` draws one row's content.
    pub fn new(
        id: impl Into<ElementId>,
        item_count: usize,
        row_key: impl Fn(usize) -> ElementId + 'static,
        render_row: impl Fn(usize, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            item_count,
            row_key: Rc::new(row_key),
            render_row: Rc::new(render_row),
            scroll_handle: None,
        }
    }

    /// Follow `handle` instead of the list's own scroll state, so the caller can scroll
    /// to an item (`UniformListScrollHandle::scroll_to_item`) and keep its scroll
    /// position across frames. Optional: a list no one scrolls programmatically needs
    /// no handle.
    pub fn track_scroll(mut self, handle: UniformListScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
        self
    }
}

impl RenderOnce for VirtualList {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let row_key = self.row_key;
        let render_row = self.render_row;
        let list = uniform_list(self.id, self.item_count, move |range, window, cx| {
            range
                .map(|index| {
                    // The id and the test selector are the item's key, so a row keeps its
                    // identity when rows scroll past or the data is reordered. The
                    // selector follows the same test-hook pattern as `components/tab.rs`.
                    let key = row_key(index);
                    let selector = key.to_string();
                    div()
                        .id(key)
                        .debug_selector(move || selector.clone())
                        .child(render_row(index, window, cx))
                })
                .collect()
        })
        .size_full();
        match self.scroll_handle {
            Some(handle) => list.track_scroll(&handle).into_any_element(),
            None => list.into_any_element(),
        }
    }
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
