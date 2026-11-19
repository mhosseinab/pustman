//! Dock panel registration: the [`Panel`] feature crates implement and the
//! [`PanelHandle`] seam the docks store, so a new panel lands with its own crate and
//! never needs a workspace edit (S22c).

use gpui::{AnyView, App, Entity, EntityId, FocusHandle, Focusable, Window};
use ui::prelude::*;

use crate::Workspace;
use crate::dock::DockPosition;

/// A dock panel: the shape a feature crates implements to get its view into a dock.
///
/// [`Focusable`] supplies the focus handle — the dock's selector focuses the panel it
/// activates through it.
pub trait Panel: Render + Focusable {
    /// The panel's stable id: it names the dock's selector entry and its hitbox, so
    /// both survive panels being added or replaced.
    fn panel_id(&self) -> &'static str;

    /// The panel's title — its selector entry's label and accessible name.
    fn title(&self, cx: &App) -> SharedString;

    /// The panel's selector icon.
    fn icon(&self) -> IconName;

    /// Which dock this panel belongs in.
    fn position(&self, cx: &App) -> DockPosition;
}

/// The type-erased panel a [`crate::Dock`] stores: enough to identify, render and focus
/// a registered panel without knowing its concrete type.
pub trait PanelHandle {
    /// See [`Panel::panel_id`].
    fn panel_id(&self, cx: &App) -> &'static str;

    /// See [`Panel::title`].
    fn title(&self, cx: &App) -> SharedString;

    /// See [`Panel::icon`].
    fn icon(&self, cx: &App) -> IconName;

    /// See [`Panel::position`].
    fn position(&self, cx: &App) -> DockPosition;

    /// The panel as a view the dock can render.
    fn to_any_view(&self) -> AnyView;

    /// The panel's focus handle.
    fn focus_handle(&self, cx: &App) -> FocusHandle;

    /// The panel entity's id — how tests tell registered panels apart.
    fn entity_id(&self) -> EntityId;
}

impl<T: Panel> PanelHandle for Entity<T> {
    fn panel_id(&self, cx: &App) -> &'static str {
        self.read(cx).panel_id()
    }

    fn title(&self, cx: &App) -> SharedString {
        self.read(cx).title(cx)
    }

    fn icon(&self, cx: &App) -> IconName {
        self.read(cx).icon()
    }

    fn position(&self, cx: &App) -> DockPosition {
        self.read(cx).position(cx)
    }

    fn to_any_view(&self) -> AnyView {
        self.clone().into()
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn entity_id(&self) -> EntityId {
        Entity::entity_id(self)
    }
}

impl Workspace {
    /// Register `panel` with its dock: a feature crate hands over its panel and the
    /// workspace routes it by [`Panel::position`], so a new panel never needs a
    /// workspace edit (S22c).
    ///
    /// Contract: registration never moves keyboard focus — the workspace keeps it
    /// until the user activates the panel's selector entry. `window` stays in the seam
    /// so a later step can hand focus over on registration without changing callers.
    pub fn register_panel<T: Panel>(
        &mut self,
        panel: Entity<T>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = panel.read(cx).position(cx);
        let dock = match position {
            DockPosition::Left => &self.left_dock,
            DockPosition::Bottom => &self.bottom_dock,
        };
        dock.update(cx, |dock, cx| dock.add_panel(Box::new(panel), cx));
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
