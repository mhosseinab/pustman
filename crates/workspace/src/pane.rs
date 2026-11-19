//! The center pane: the region open items live in (PRD §3, REQ-UI-02). It holds one tab
//! per item — the tab strip and its close, pin and reorder behaviour live in
//! [`crate::pane_tabs`] — knows which item is active, and shows [`Welcome`] when it
//! holds none.

use gpui::{
    AnyView, Context, Entity, EntityId, IntoElement, ParentElement, Render, Subscription, Window,
};
use ui::prelude::*;

use crate::item::{AnyViewItem, ItemHandle};
use crate::pane_tabs::PaneTab;
use crate::welcome::Welcome;

/// The pane's stable element id: fixed, one singleton region of the shell.
const PANE_ID: &str = "center-pane";

/// The items the workspace is showing as tabs, and which of them is active.
pub struct Pane {
    // Crate-visible so the tab methods in [`crate::pane_tabs`] can extend this struct
    // from a sibling module.
    pub(crate) tabs: Vec<PaneTab>,
    pub(crate) active_id: Option<EntityId>,
    pub(crate) tab_bar: Entity<TabBar>,
    pub(crate) welcome: Entity<Welcome>,
    pub(crate) _tab_bar_subscription: Subscription,
}

impl Pane {
    /// An empty pane — it draws its [`Welcome`] empty state until an item is added.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tab_bar = cx.new(TabBar::new);
        let _tab_bar_subscription = cx.subscribe(&tab_bar, Self::on_tab_bar_event);
        Self {
            tabs: Vec::new(),
            active_id: None,
            tab_bar,
            welcome: cx.new(Welcome::new),
            _tab_bar_subscription,
        }
    }

    /// Add a plain view to the pane as a tab. The added item becomes the active one.
    pub fn add_item(&mut self, item: impl Into<AnyView>, cx: &mut Context<Self>) {
        self.push_tab(Box::new(AnyViewItem::new(item.into())), None, cx);
    }

    /// The item entities this pane holds as tabs, in tab order.
    pub fn item_ids(&self) -> Vec<EntityId> {
        self.tabs.iter().map(|tab| tab.handle.entity_id()).collect()
    }

    /// The item that is showing, if the pane holds any.
    pub fn active_item(&self) -> Option<&dyn ItemHandle> {
        self.tabs
            .iter()
            .find(|tab| self.active_id == Some(tab.handle.entity_id()))
            .map(|tab| tab.handle.as_ref())
    }

    /// The index of the item that is showing.
    pub fn active_index(&self) -> usize {
        self.active_position().unwrap_or(0)
    }

    /// Show the item at `index`; an index past the end is ignored.
    pub fn set_active_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let entity_id = match self.tabs.get(index) {
            Some(tab) => tab.handle.entity_id(),
            None => return,
        };
        self.activate_item(entity_id, cx);
    }
}

impl Render for Pane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_view = self.active_item().map(|item| item.to_any_view());
        v_flex()
            .id(PANE_ID)
            .flex_1()
            .h_full()
            .min_w_0()
            .bg(ElevationIndex::Surface.bg(cx))
            // The tab strip only exists while the pane holds tabs; the empty state is
            // the Welcome item this pane owns, not a tab.
            .children((!self.tabs.is_empty()).then(|| self.tab_bar.clone()))
            .map(|this| match active_view {
                Some(item) => this.child(item),
                None => this.child(self.welcome.clone()),
            })
            .into_any_element()
    }
}
