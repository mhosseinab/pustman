//! The tab strip on [`Pane`] (REQ-UI-02): one tab per item — close, pin, reorder and
//! the dirty dot — built on [`ui::TabBar`], plus the handlers behind `NewTab`, `CloseTab`,
//! `NextTab` and `PreviousTab`.
//!
//! Every change to the strip happens here, outside `render`: the tab list is rebuilt with
//! [`TabBar::set_tabs`] after add/close/move/pin/activate and when an item notifies.

use gpui::{Context, Entity, EntityId, Subscription};
use ui::prelude::*;

use crate::Pane;
use crate::item::{Item, ItemHandle};
use crate::welcome::Welcome;

/// One tab on the pane: the item behind it, and the state a tab carries beyond the item.
pub(crate) struct PaneTab {
    pub(crate) handle: Box<dyn ItemHandle>,
    pub(crate) pinned: bool,
    /// Rebuilds the strip when the item notifies (title or dirty state changed). The
    /// plain-view path has no entity to observe, hence `Option`.
    pub(crate) _item_observer: Option<Subscription>,
}

impl Pane {
    /// Add a real [`Item`] to the pane as a tab. The added item becomes the active one,
    /// and its tab follows the item: a title or dirty change rebuilds the strip.
    pub fn add_tab_item<T: Item>(&mut self, item: Entity<T>, cx: &mut Context<Self>) {
        let observer = cx.observe(&item, |pane, _item, cx| pane.rebuild_tabs(cx));
        self.push_tab(Box::new(item), Some(observer), cx);
    }

    /// Open a [`Welcome`] tab: what `NewTab` falls back to when no crate answers
    /// `NewRequest` (see `Workspace::new_tab`).
    pub fn new_tab(&mut self, cx: &mut Context<Self>) {
        let welcome = cx.new(Welcome::new);
        self.add_tab_item(welcome, cx);
    }

    /// Close the active tab (`CloseTab`); a pinned tab is never closed (Decision).
    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        let entity_id = match self.active_id {
            Some(entity_id) => entity_id,
            None => return,
        };
        self.close_tab(entity_id, cx);
    }

    /// Close the tab of `entity_id`: a pinned tab survives (Decision), and an active tab
    /// hands over to its right neighbour, else its left one. Closing the last tab leaves
    /// the pane on the [`Welcome`] empty state.
    pub fn close_tab(&mut self, entity_id: EntityId, cx: &mut Context<Self>) {
        let index = match self.position_of(entity_id) {
            Some(index) => index,
            None => return,
        };
        if self.tabs.get(index).is_some_and(|tab| tab.pinned) {
            return;
        }
        self.tabs.remove(index);
        if self.active_id == Some(entity_id) {
            // After the removal, `index` holds the old right neighbour; a tab closed at
            // the end leaves its left one.
            self.active_id = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(|tab| tab.handle.entity_id());
        }
        self.rebuild_tabs(cx);
        self.sync_tab_bar_selection(cx);
        cx.notify();
    }

    /// Activate the next tab (`NextTab`), wrapping around. Zero or one tab: nothing.
    pub fn activate_next_tab(&mut self, cx: &mut Context<Self>) {
        let len = self.tabs.len();
        if len < 2 {
            return;
        }
        self.activate_tab_at((self.active_position().unwrap_or(0) + 1) % len, cx);
    }

    /// Activate the previous tab (`PreviousTab`), wrapping around. Zero or one tab:
    /// nothing.
    pub fn activate_previous_tab(&mut self, cx: &mut Context<Self>) {
        let len = self.tabs.len();
        if len < 2 {
            return;
        }
        self.activate_tab_at((self.active_position().unwrap_or(0) + len - 1) % len, cx);
    }

    /// Pin or unpin the tab of `entity_id`. Pinned tabs sort before the unpinned ones,
    /// keeping their relative order.
    pub fn set_pinned(&mut self, entity_id: EntityId, pinned: bool, cx: &mut Context<Self>) {
        let index = match self.position_of(entity_id) {
            Some(index) => index,
            None => return,
        };
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.pinned = pinned;
        }
        self.tabs.sort_by_key(|tab| !tab.pinned);
        self.rebuild_tabs(cx);
        self.sync_tab_bar_selection(cx);
        cx.notify();
    }

    /// Move the tab at `from` to `to`, the reorder drag's target. Indices past the end
    /// are ignored, and the destination is clamped into the tab's own group so pinned
    /// tabs stay before unpinned ones.
    pub fn move_item(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let pinned = match self.tabs.get(from) {
            Some(tab) => tab.pinned,
            None => return,
        };
        let pinned_count = self.tabs.iter().filter(|tab| tab.pinned).count();
        let (region_start, region_end) = if pinned {
            (0, pinned_count)
        } else {
            (pinned_count, self.tabs.len())
        };
        // The tab itself is in its region, so the region is never empty.
        let to = to.clamp(region_start, region_end.saturating_sub(1));
        if to == from {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.rebuild_tabs(cx);
        self.sync_tab_bar_selection(cx);
        cx.notify();
    }

    /// Push `handle` as a new tab and activate it.
    pub(crate) fn push_tab(
        &mut self,
        handle: Box<dyn ItemHandle>,
        observer: Option<Subscription>,
        cx: &mut Context<Self>,
    ) {
        let entity_id = handle.entity_id();
        self.tabs.push(PaneTab {
            handle,
            pinned: false,
            _item_observer: observer,
        });
        self.activate_item(entity_id, cx);
    }

    /// Make `entity_id`'s tab the active one and restage the strip around it.
    pub(crate) fn activate_item(&mut self, entity_id: EntityId, cx: &mut Context<Self>) {
        self.active_id = Some(entity_id);
        self.rebuild_tabs(cx);
        self.sync_tab_bar_selection(cx);
        cx.notify();
    }

    /// Where `entity_id` sits in the tab order, if it is open here.
    fn position_of(&self, entity_id: EntityId) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.handle.entity_id() == entity_id)
    }

    /// The active tab's place in the tab order, if the pane holds one.
    pub(crate) fn active_position(&self) -> Option<usize> {
        self.active_id
            .and_then(|entity_id| self.position_of(entity_id))
    }

    /// Activate the tab at `index`, if there is one. The single entry point for the tab
    /// bar's events: the active tab is a no-op, which also keeps the bar's own selection
    /// echoes (`set_selected` emits `Selected` again) from cycling.
    fn activate_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let entity_id = match self.tabs.get(index) {
            Some(tab) => tab.handle.entity_id(),
            None => return,
        };
        if self.active_id == Some(entity_id) {
            return;
        }
        self.activate_item(entity_id, cx);
    }

    /// The tab bar's [`TabBarEvent`]s: moving the selection with the arrows or activating
    /// a tab with the mouse switches the pane to that tab.
    pub(crate) fn on_tab_bar_event(
        &mut self,
        tab_bar: Entity<TabBar>,
        event: &TabBarEvent,
        cx: &mut Context<Self>,
    ) {
        let index = match event {
            TabBarEvent::Selected { index } | TabBarEvent::Activated { index } => *index,
        };
        // The event carries the index as of the moment the bar emitted it, but effects
        // flush after the whole update: a later selection — another gesture, or this
        // pane's own `sync_tab_bar_selection` echo — supersedes it. The bar's current
        // selection is the truth; an event that no longer describes it is stale and is
        // dropped, so queued echoes can never flip the active tab back and forth.
        if index != tab_bar.read(cx).selected() {
            return;
        }
        self.activate_tab_at(index, cx);
    }

    /// Rebuild the strip from the pane's tabs: titles, dirty dots, pins and close
    /// buttons. Called after every change to the tabs or the active item — never from
    /// `render`.
    pub(crate) fn rebuild_tabs(&mut self, cx: &mut Context<Self>) {
        let active_id = self.active_id;
        let pane = cx.weak_entity();
        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .map(|tab| {
                let entity_id = tab.handle.entity_id();
                let pinned = tab.pinned;
                let title = tab.handle.tab_title(cx);
                let dirty = tab.handle.is_dirty(cx);
                let tab = Tab::new(("pane-tab", entity_id), title)
                    .dirty(dirty)
                    .pinned(pinned)
                    .toggle_state(active_id == Some(entity_id));
                // A pinned tab keeps its close button out of the strip: `CloseTab` and
                // this button both refuse to close a pinned tab, and a button that never
                // does anything would be a dead end (Decision).
                if pinned {
                    tab
                } else {
                    tab.on_close({
                        let pane = pane.clone();
                        move |_, _, cx| {
                            // The pane may have been released with its window; its tabs
                            // went with it and the click then has nothing left to close.
                            if let Err(_released) =
                                pane.update(cx, |pane, cx| pane.close_tab(entity_id, cx))
                            {
                                // Handled: there is nothing to close.
                            }
                        }
                    })
                }
            })
            .collect();
        self.tab_bar.update(cx, |bar, cx| bar.set_tabs(tabs, cx));
    }

    /// Point the tab bar's selection at the active tab. `TabBar::set_selected` echoes a
    /// `Selected` event back; [`Self::on_tab_bar_event`] drops the echo because it
    /// describes the selection the pane just made, not a new gesture.
    fn sync_tab_bar_selection(&self, cx: &mut Context<Self>) {
        let index = self.active_position().unwrap_or(0);
        self.tab_bar
            .update(cx, |bar, cx| bar.set_selected(index, cx));
    }
}

#[cfg(test)]
#[path = "pane_tabs_tests.rs"]
mod tests;
