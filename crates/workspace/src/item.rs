//! The [`Item`] trait — what a pane tab's content must be — and the type-erased
//! [`ItemHandle`] the pane stores one of per tab, so one pane can hold items of
//! different types behind one interface.

use gpui::{AnyView, App, Entity, EntityId, Render, SharedString};

/// What can live in a pane tab: anything that renders and can tell the tab strip its
/// title and whether it holds unsaved changes (REQ-UI-02).
pub trait Item: Render {
    /// The title the item's tab shows.
    fn tab_title(&self, cx: &App) -> SharedString;

    /// Whether the item has unsaved changes — the tab shows the dirty dot.
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    /// The item's draft as text, the hook the workspace store will persist open tabs
    /// through. `None` when there is nothing to persist.
    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        None
    }
}

/// A type-erased [`Item`] handle: what [`crate::Pane`] stores one of per tab.
///
/// Implemented for `Entity<T: Item>` and for [`AnyViewItem`] (the plain-view path
/// `Pane::add_item` takes), so both end up in the same tab strip.
pub trait ItemHandle {
    /// The item entity's stable id — a tab's element id comes from this, never from the
    /// tab's index.
    fn entity_id(&self) -> EntityId;

    /// The item as a view, for the pane to render.
    fn to_any_view(&self) -> AnyView;

    /// The title the item's tab shows.
    fn tab_title(&self, cx: &App) -> SharedString;

    /// Whether the item's tab shows the dirty dot.
    fn is_dirty(&self, cx: &App) -> bool;

    /// The item's draft as text, for persisting open tabs.
    fn serialize_draft(&self, cx: &App) -> Option<String>;

    /// A second handle to the same item.
    fn boxed_clone(&self) -> Box<dyn ItemHandle>;
}

impl<T: Item> ItemHandle for Entity<T> {
    fn entity_id(&self) -> EntityId {
        Entity::entity_id(self)
    }

    fn to_any_view(&self) -> AnyView {
        self.clone().into()
    }

    fn tab_title(&self, cx: &App) -> SharedString {
        self.read(cx).tab_title(cx)
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.read(cx).is_dirty(cx)
    }

    fn serialize_draft(&self, cx: &App) -> Option<String> {
        self.read(cx).serialize_draft(cx)
    }

    fn boxed_clone(&self) -> Box<dyn ItemHandle> {
        Box::new(self.clone())
    }
}

/// The plain-view path [`crate::Pane::add_item`] takes: a view that is not an [`Item`]
/// still gets a tab — titled "Untitled", never dirty, nothing to serialize.
pub struct AnyViewItem {
    view: AnyView,
}

impl AnyViewItem {
    /// Wrap a plain view as a tab's content.
    pub fn new(view: AnyView) -> Self {
        Self { view }
    }
}

impl ItemHandle for AnyViewItem {
    fn entity_id(&self) -> EntityId {
        self.view.entity_id()
    }

    fn to_any_view(&self) -> AnyView {
        self.view.clone()
    }

    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Untitled")
    }

    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    fn serialize_draft(&self, _cx: &App) -> Option<String> {
        None
    }

    fn boxed_clone(&self) -> Box<dyn ItemHandle> {
        Box::new(Self {
            view: self.view.clone(),
        })
    }
}
