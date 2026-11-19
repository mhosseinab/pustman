//! The center pane: the region open request items live in (PRD §3, REQ-UI-02). It holds
//! items and knows which one is active; the tab strip that switches between them arrives
//! with the `Item` trait in a later step.

use gpui::{AnyView, Context, IntoElement, ParentElement, Render, Window};
use ui::prelude::*;

/// The pane's stable element id: fixed, one singleton region of the shell.
const PANE_ID: &str = "center-pane";

/// The items the workspace is showing, and which of them is active.
pub struct Pane {
    items: Vec<AnyView>,
    active_index: usize,
}

impl Pane {
    /// An empty pane — it draws its empty state until an item is added.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            items: Vec::new(),
            active_index: 0,
        }
    }

    /// Add an item to the pane. The first item a pane holds becomes its active one.
    pub fn add_item(&mut self, item: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.items.push(item.into());
        cx.notify();
    }

    /// The items this pane holds, in the order they were added.
    pub fn items(&self) -> &[AnyView] {
        &self.items
    }

    /// The item that is showing, if the pane holds any.
    pub fn active_item(&self) -> Option<&AnyView> {
        self.items.get(self.active_index)
    }

    /// The index of the item that is showing.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Show the item at `index`; an index past the end is ignored.
    pub fn set_active_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.items.get(index).is_none() {
            return;
        }
        self.active_index = index;
        cx.notify();
    }
}

impl Render for Pane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_item = self.active_item().cloned();
        v_flex()
            .id(PANE_ID)
            .flex_1()
            .h_full()
            .min_w_0()
            .bg(ElevationIndex::Surface.bg(cx))
            .map(|this| match active_item {
                Some(item) => this.child(item),
                None => this
                    .justify_center()
                    .items_center()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(Label::new("No open requests").color(Color::Muted))
                    .child(
                        Label::new("Open one from the collections panel, or create a new request.")
                            .color(Color::Hint)
                            .truncate(),
                    ),
            })
            .into_any_element()
    }
}
