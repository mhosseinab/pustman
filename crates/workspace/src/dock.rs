//! The dock: the container a workspace edge's panels live in (PRD §3).
//!
//! A dock is pinned to one edge of the workspace, is open or closed, and holds at most
//! one panel. The panel registration that fills the docks lands in a later step, so an
//! open dock with no panel renders an empty container of its fixed size — and a closed
//! dock renders nothing at all, so the shell lays out around it.

use gpui::{AnyView, Context, IntoElement, ParentElement, Render, Window, div};
use ui::prelude::*;

/// The left dock's width: the redesign pack's `layout.collectionSidebar`, 248 px.
const LEFT_DOCK_WIDTH_PX: f32 = 248.;

/// The bottom dock's height. The pack fixes no console height; REQ-UI-01/04 make docks
/// resizable and persist their size, so until then the console sits in a 200 px strip.
const BOTTOM_DOCK_HEIGHT_PX: f32 = 200.;

/// Which edge of the workspace a dock is pinned to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPosition {
    /// The left edge: collections, environments and history (REQ-UI-01).
    Left,
    /// The bottom edge: the console (REQ-UI-04).
    Bottom,
}

impl DockPosition {
    /// The dock's stable element id and test selector: fixed strings, one per singleton
    /// region of the shell.
    pub fn element_id(self) -> &'static str {
        match self {
            Self::Left => "left-dock",
            Self::Bottom => "bottom-dock",
        }
    }
}

/// One edge of the workspace: open or closed, holding at most one panel.
pub struct Dock {
    position: DockPosition,
    is_open: bool,
    panel: Option<AnyView>,
}

impl Dock {
    /// A dock at `position`, open — the workspace opens with both docks visible.
    pub fn new(position: DockPosition, _cx: &mut Context<Self>) -> Self {
        Self {
            position,
            is_open: true,
            panel: None,
        }
    }

    /// Which edge this dock is pinned to.
    pub fn position(&self) -> DockPosition {
        self.position
    }

    /// Whether the dock is showing.
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Show or hide the dock.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.is_open = open;
        cx.notify();
    }

    /// Flip between hidden and showing.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.set_open(!self.is_open, cx);
    }

    /// The panel this dock shows, if one is registered.
    pub fn panel(&self) -> Option<&AnyView> {
        self.panel.as_ref()
    }

    /// Put a panel in this dock; `None` empties it.
    pub fn set_panel(&mut self, panel: Option<AnyView>, cx: &mut Context<Self>) {
        self.panel = panel;
        cx.notify();
    }
}

impl Render for Dock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open {
            return div().into_any_element();
        }

        let colors = cx.theme().colors();
        let element_id = self.position.element_id();
        let panel = self.panel.clone();
        div()
            .id(element_id)
            .debug_selector(|| element_id.to_owned())
            .flex_none()
            .bg(colors.panel_background)
            .map(|this| match self.position {
                DockPosition::Left => this
                    .w(px_to_rems(LEFT_DOCK_WIDTH_PX))
                    .h_full()
                    .border_r_1()
                    .border_color(colors.border_variant),
                DockPosition::Bottom => this
                    .w_full()
                    .h(px_to_rems(BOTTOM_DOCK_HEIGHT_PX))
                    .border_t_1()
                    .border_color(colors.border_variant),
            })
            .children(panel)
            .into_any_element()
    }
}
