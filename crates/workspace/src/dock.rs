//! The dock: the container a workspace edge's panels live in (PRD §3).
//!
//! A dock is pinned to one edge of the workspace, is open or closed, and holds the
//! [`PanelHandle`]s feature crates register for its edge: a full-width panel header row
//! above the active one — the panel's title in the default text colour at the left, the
//! dock's actions at the right — like the workbench blueprint's sidebar and Console bar
//! headers. An open dock with no panel renders an empty container of its fixed size — and
//! a closed dock renders nothing at all, so the shell lays out around it.

use gpui::{AnyView, Context, IntoElement, ParentElement, Render, Window, div};
use std::cmp::Ordering;
use ui::prelude::*;

use crate::panel::PanelHandle;

/// The left dock's width: the redesign pack's `layout.collectionSidebar`, 248 px.
const LEFT_DOCK_WIDTH_PX: f32 = 248.;

/// The bottom dock's height. The pack fixes no console height; REQ-UI-01/04 make docks
/// resizable and persist their size, so until then the console sits in a 200 px strip.
const BOTTOM_DOCK_HEIGHT_PX: f32 = 200.;

/// The smallest and largest size a dock may take. A restored size is local, untrusted
/// data (SQLite rows), so it is clamped into the range a usable dock can have.
const MIN_DOCK_SIZE_PX: f32 = 64.;
const MAX_DOCK_SIZE_PX: f32 = 2048.;

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

    /// The size this dock renders at until something resizes or restores another: its
    /// width for [`DockPosition::Left`], its height for [`DockPosition::Bottom`].
    pub fn default_size_px(self) -> f32 {
        match self {
            Self::Left => LEFT_DOCK_WIDTH_PX,
            Self::Bottom => BOTTOM_DOCK_HEIGHT_PX,
        }
    }
}

/// One edge of the workspace: open or closed, holding the panels registered for it and
/// showing one of them at a time behind its panel header row.
pub struct Dock {
    position: DockPosition,
    is_open: bool,
    size_px: f32,
    panels: Vec<Box<dyn PanelHandle>>,
    active_panel: Option<usize>,
}

impl Dock {
    /// A dock at `position`, open and at its default size — the workspace opens with both
    /// docks visible.
    pub fn new(position: DockPosition, _cx: &mut Context<Self>) -> Self {
        Self {
            position,
            is_open: true,
            size_px: position.default_size_px(),
            panels: Vec::new(),
            active_panel: None,
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

    /// The size this dock renders at (REQ-UI-01/04): its width for
    /// [`DockPosition::Left`], its height for [`DockPosition::Bottom`].
    pub fn size_px(&self) -> f32 {
        self.size_px
    }

    /// Resize the dock (REQ-UI-01/04). A size read back from the local store is clamped
    /// — see [`MIN_DOCK_SIZE_PX`].
    pub fn set_size_px(&mut self, size_px: f32, cx: &mut Context<Self>) {
        self.size_px = size_px.clamp(MIN_DOCK_SIZE_PX, MAX_DOCK_SIZE_PX);
        cx.notify();
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

    /// The panels this dock holds, in registration order.
    pub fn panels(&self) -> &[Box<dyn PanelHandle>] {
        &self.panels
    }

    /// The active panel's view, if this dock holds one.
    pub fn panel(&self) -> Option<AnyView> {
        self.active_panel
            .and_then(|index| self.panels.get(index))
            .map(|panel| panel.to_any_view())
    }

    /// Put a panel in this dock; the first panel added becomes the active one. A panel
    /// registered under an id this dock already holds replaces that entry in place —
    /// same slot, same selection — so re-registration can never duplicate an entry.
    pub fn add_panel(&mut self, panel: Box<dyn PanelHandle>, cx: &mut Context<Self>) {
        let panel_id = panel.panel_id(cx);
        match self
            .panels
            .iter()
            .position(|registered| registered.panel_id(cx) == panel_id)
        {
            Some(index) => {
                if let Some(registered) = self.panels.get_mut(index) {
                    *registered = panel;
                }
            }
            None => {
                self.panels.push(panel);
                if self.active_panel.is_none() {
                    self.active_panel = Some(self.panels.len().saturating_sub(1));
                }
            }
        }
        cx.notify();
    }

    /// Drop the entry registered under `panel_id`, if this dock holds one: the other
    /// half of moving a panel to another dock by re-registering its id there (S22c
    /// review) — without it, this dock keeps an entry for a panel that now lives
    /// elsewhere. The selection follows the entry out: the panel that takes its slot
    /// becomes active, or the dock empties.
    pub fn remove_panel(&mut self, panel_id: &str, cx: &mut Context<Self>) {
        let index = match self
            .panels
            .iter()
            .position(|registered| registered.panel_id(cx) == panel_id)
        {
            Some(index) => index,
            None => return,
        };
        self.panels.remove(index);
        self.active_panel = self
            .active_panel
            .and_then(|active| match active.cmp(&index) {
                Ordering::Less => Some(active),
                Ordering::Equal => Some(index.min(self.panels.len().checked_sub(1)?)),
                Ordering::Greater => Some(active - 1),
            });
        cx.notify();
    }

    /// Show the panel registered under `panel_id` and move keyboard focus into it. An
    /// unknown id is ignored — a stale click must not panic or clear the selection.
    pub fn activate_panel(&mut self, panel_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .panels
            .iter()
            .position(|registered| registered.panel_id(cx) == panel_id)
        else {
            return;
        };
        self.active_panel = Some(index);
        cx.notify();
        if let Some(panel) = self.panels.get(index) {
            let focus_handle = panel.focus_handle(cx);
            window.focus(&focus_handle, cx);
        }
    }

    /// The panel header row above the active panel: the active panel's title in the
    /// default text colour at the left and the dock's actions at the right. The other
    /// panels this dock holds are actions too — their entries switch to them. A dock
    /// draws the row only while it holds at least one panel.
    fn panel_header(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        if self.panels.is_empty() {
            return None;
        }
        let colors = cx.theme().colors();
        let active_index = self.active_panel.unwrap_or(0);
        let mut title = None;
        let mut actions = Vec::new();
        for (index, panel) in self.panels.iter().enumerate() {
            let panel_id = panel.panel_id(cx);
            let entry = panel_entry(
                panel_id,
                panel.title(cx),
                panel.icon(cx),
                index == active_index,
                // The handler runs when the click is dispatched — never inside
                // render — and reaches the dock through `cx.listener`, so no view
                // is updated re-entrantly.
                cx.listener(move |dock, _: &ClickEvent, window, cx| {
                    dock.activate_panel(panel_id, window, cx);
                }),
            );
            if index == active_index {
                title = Some(entry);
            } else {
                actions.push(entry);
            }
        }
        let position = self.position;
        Some(
            h_flex()
                .w_full()
                .flex_none()
                .px(DynamicSpacing::Base06.rems(cx))
                .py(DynamicSpacing::Base04.rems(cx))
                .gap(DynamicSpacing::Base02.rems(cx))
                .border_b_1()
                .border_color(colors.border_variant)
                .debug_selector(move || dock_header_selector(position))
                .children(title)
                .child(div().flex_1())
                .children(actions)
                .child(self.hide_action(cx)),
        )
    }

    /// The panel header's rightmost action: hide the dock — what `ToggleSidebar` /
    /// `ToggleConsole` do from the keyboard. Without it the dock could only be hidden
    /// from the keyboard.
    fn hide_action(&self, cx: &Context<Self>) -> impl IntoElement {
        let selector = dock_hide_selector(self.position);
        let (icon, label) = match self.position {
            DockPosition::Left => (IconName::ChevronLeft, "Hide sidebar"),
            DockPosition::Bottom => (IconName::ChevronDown, "Hide console"),
        };
        div().debug_selector(move || selector.clone()).child(
            IconButton::new(
                format!("dock-hide-button-{}", self.position.element_id()),
                icon,
                Tooltip::text(label),
            )
            .on_click(cx.listener(|dock, _event, _window, cx| dock.set_open(false, cx))),
        )
    }
}

impl Render for Dock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open {
            return div().into_any_element();
        }

        let header = self.panel_header(cx);
        let active_panel = self.panel();
        let colors = cx.theme().colors();
        let element_id = self.position.element_id();
        v_flex()
            .id(element_id)
            .debug_selector(|| element_id.to_owned())
            .flex_none()
            .bg(colors.panel_background)
            .map(|this| match self.position {
                DockPosition::Left => this
                    .w(px_to_rems(self.size_px))
                    .h_full()
                    .border_r_1()
                    .border_color(colors.border_variant),
                DockPosition::Bottom => this
                    .w_full()
                    .h(px_to_rems(self.size_px))
                    .border_t_1()
                    .border_color(colors.border_variant),
            })
            .children(header)
            .children(active_panel.map(|panel| div().flex_1().min_h_0().child(panel)))
            .into_any_element()
    }
}

/// The debug selector [`panel_entry`] puts on one panel's entry — the one string
/// tests hand to `VisualTestContext::debug_bounds` — and the entry's element id.
pub(crate) fn dock_panel_selector(panel_id: &str) -> String {
    format!("dock-panel-{panel_id}")
}

/// The debug selector the dock's panel header row carries.
pub(crate) fn dock_header_selector(position: DockPosition) -> String {
    format!("dock-header-{}", position.element_id())
}

/// The debug selector the panel header's hide action carries.
pub(crate) fn dock_hide_selector(position: DockPosition) -> String {
    format!("dock-hide-{}", position.element_id())
}

/// One entry in a dock's panel header: the active panel's title — bare text in the
/// default text colour — or, for the dock's other panels, a right-hand action that
/// switches to them. The hitbox is keyed by the panel id — never by an index — so hover,
/// focus and test hitboxes survive panels being added or replaced.
fn panel_entry(
    panel_id: &'static str,
    title: SharedString,
    icon: IconName,
    is_title: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    // The title is not a tinted selector chip: `Transparent` keeps the default text
    // colour (`ButtonStyle::text_color`), and `Medium` makes it a header row, not a
    // tiny pill.
    let style = if is_title {
        ButtonStyle::Transparent
    } else {
        ButtonStyle::Subtle
    };
    h_flex()
        .id(dock_panel_selector(panel_id))
        .debug_selector(move || dock_panel_selector(panel_id))
        .child(
            Button::new(format!("dock-panel-button-{panel_id}"), title)
                .start_icon(Icon::new(icon))
                .style(style)
                .size(ButtonSize::Medium)
                .on_click(on_click),
        )
}

#[cfg(test)]
#[path = "dock_header_tests.rs"]
mod header_tests;
