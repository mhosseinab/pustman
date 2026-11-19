//! The status bar: the strip along the bottom edge of the workspace (PRD §3). Other
//! crates drop their readouts into it — environment, cookies, console — through
//! [`StatusBar::add_left_item`] and [`StatusBar::add_right_item`].

use gpui::{AnyView, Context, ElementId, EntityId, IntoElement, ParentElement, Render, Window};
use ui::prelude::*;

/// The status bar's stable element id: fixed, one singleton region of the shell.
const STATUS_BAR_ID: &str = "status-bar";

/// Debug selectors for the two item groups: tests can tell the anchored-left side from
/// the anchored-right side.
pub(crate) const LEFT_ITEMS_SELECTOR: &str = "status-bar-left";
pub(crate) const RIGHT_ITEMS_SELECTOR: &str = "status-bar-right";

/// The status bar's height: the redesign pack's `layout.statusBar`, 28 px. The toast
/// layer anchors its stack to it (`toast_layer.rs`), so the height lives here — the one
/// place that knows it.
pub(crate) const STATUS_BAR_HEIGHT_PX: f32 = 28.;

/// The strip along the bottom edge: items anchored left, items anchored right.
pub struct StatusBar {
    left_items: Vec<AnyView>,
    right_items: Vec<AnyView>,
}

impl StatusBar {
    /// An empty status bar, waiting for items.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            left_items: Vec::new(),
            right_items: Vec::new(),
        }
    }

    /// Add an item to the left side.
    pub fn add_left_item(&mut self, view: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.left_items.push(view.into());
        cx.notify();
    }

    /// Add an item to the right side.
    pub fn add_right_item(&mut self, view: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.right_items.push(view.into());
        cx.notify();
    }

    /// The items anchored to the left, in the order they were added.
    pub fn left_items(&self) -> &[AnyView] {
        &self.left_items
    }

    /// The items anchored to the right, in the order they were added.
    pub fn right_items(&self) -> &[AnyView] {
        &self.right_items
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        h_flex()
            .id(STATUS_BAR_ID)
            .debug_selector(|| STATUS_BAR_ID.to_owned())
            .w_full()
            .h(px_to_rems(STATUS_BAR_HEIGHT_PX))
            .flex_none()
            .bg(colors.status_bar_background)
            .border_t_1()
            .border_color(colors.border_variant)
            .px(DynamicSpacing::Base04.rems(cx))
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                h_flex()
                    .debug_selector(|| LEFT_ITEMS_SELECTOR.to_owned())
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .children(self.left_items.iter().cloned().map(status_item)),
            )
            // Push the right-side items to the far edge.
            .child(h_flex().flex_1())
            .child(
                h_flex()
                    .debug_selector(|| RIGHT_ITEMS_SELECTOR.to_owned())
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .children(self.right_items.iter().cloned().map(status_item)),
            )
    }
}

/// One status item in its stable place. The element id and the debug selector both come
/// from the item's entity, so hover, focus and test hitboxes survive items being added or
/// reordered — and `VisualTestContext::debug_bounds` can find the rendered item.
fn status_item(item: AnyView) -> impl IntoElement {
    let entity_id = item.entity_id();
    h_flex()
        .id(ElementId::from(("status-item", entity_id)))
        .debug_selector(move || status_item_selector(entity_id))
        .child(item)
}

/// The debug selector [`status_item`] puts on one item's rendered element — the one
/// string tests hand to `VisualTestContext::debug_bounds`.
pub(crate) fn status_item_selector(entity_id: EntityId) -> String {
    format!("status-item-{entity_id}")
}
