//! Dock panel registration: the [`Panel`] feature crates implement and the
//! [`PanelHandle`] seam the docks store, so a new panel lands with its own crate and
//! never needs a workspace edit (S22c).

use gpui::{Action, AnyView, App, Entity, EntityId, FocusHandle, Focusable, Window};
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

    /// The action that adds this panel's first kind of item — the "＋ New" its section
    /// header and the welcome pane offer — dispatched on the panel's focus handle, so
    /// the panel's own handler answers it. `None` for a panel that creates nothing.
    fn create_action(&self, _cx: &App) -> Option<Box<dyn Action>> {
        None
    }
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

    /// See [`Panel::create_action`].
    fn create_action(&self, cx: &App) -> Option<Box<dyn Action>>;

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

    fn create_action(&self, cx: &App) -> Option<Box<dyn Action>> {
        self.read(cx).create_action(cx)
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
    /// workspace edit (S22c). Re-registering an id that another dock holds *moves* the
    /// panel — the old dock drops its entry instead of keeping a stale one.
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
        let panel_id = panel.read(cx).panel_id();
        let (dock, other_dock) = match position {
            DockPosition::Left => (&self.left_dock, &self.bottom_dock),
            DockPosition::Bottom => (&self.bottom_dock, &self.left_dock),
        };
        other_dock.update(cx, |dock, cx| dock.remove_panel(panel_id, cx));
        dock.update(cx, |dock, cx| dock.add_panel(Box::new(panel), cx));
    }

    /// The "＋ New" of the left-dock panel registered under `panel_id`, if it offers one
    /// ([`Panel::create_action`]): what a button outside the panel — the welcome pane's
    /// New collection… — runs to create there.
    pub fn panel_create_action(&self, panel_id: &str, cx: &App) -> Option<Box<dyn Action>> {
        self.left_dock
            .read(cx)
            .panels()
            .iter()
            .find(|panel| panel.panel_id(cx) == panel_id)
            .and_then(|panel| panel.create_action(cx))
    }

    /// Run `panel_id`'s "＋ New" ([`Panel::create_action`]) in the panel itself: the
    /// sidebar opens on that panel with the keyboard in it, and the action is dispatched
    /// on the panel's focus handle — so the panel's own handler answers it even though
    /// the click came from outside the panel. A sidebar that was hidden is drawn first
    /// and the action follows on the next frame, once the panel is in the dispatch tree.
    pub fn run_panel_create(
        &mut self,
        panel_id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(action) = self.panel_create_action(panel_id, cx) else {
            return;
        };
        let was_open = self.left_dock.read(cx).is_open();
        self.left_dock.update(cx, |dock, cx| {
            dock.set_open(true, cx);
            dock.activate_panel(panel_id, window, cx);
        });
        let Some(focus_handle) = self
            .left_dock
            .read(cx)
            .panels()
            .iter()
            .find(|panel| panel.panel_id(cx) == panel_id)
            .map(|panel| panel.focus_handle(cx))
        else {
            return;
        };
        cx.notify();
        if was_open {
            // Deferred: this runs inside the workspace's own update, and the panel's
            // handler may reach back into the workspace (a toast, a modal).
            window.defer(cx, move |window, cx| {
                focus_handle.dispatch_action(action.as_ref(), window, cx);
            });
        } else {
            window.on_next_frame(move |window, cx| {
                focus_handle.dispatch_action(action.as_ref(), window, cx);
            });
        }
    }
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "panel_reregister_tests.rs"]
mod reregister_tests;
