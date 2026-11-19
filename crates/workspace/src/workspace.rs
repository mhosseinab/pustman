//! `workspace` — the window shell every Pustman screen lives in (PRD §3): the
//! [`Workspace`] entity with its left [`Dock`] (collections, environments, history), the
//! center [`Pane`] of request items, the bottom [`Dock`] (console) and the [`StatusBar`]
//! (`add_left_item` / `add_right_item`). Zed's `workspace` crate is GPL; these are our own
//! implementations of the shapes in
//! `.agents/skills/rust-gpui-ui-ux/references/gpui-fundamentals.md`, and every color comes
//! from `cx.theme()` so light and dark stay correct.

mod dock;
mod pane;
mod panel;
mod status_bar;

pub use dock::{Dock, DockPosition};
pub use pane::Pane;
pub use panel::{Panel, PanelHandle};
pub use status_bar::StatusBar;

use gpui::{App, Context, Entity, FocusHandle, Focusable, ParentElement, Render, Window};
use pustman_actions::workspace::{ToggleConsole, ToggleSidebar};
use ui::prelude::*;

/// The key context the workspace answers the keyboard in; `assets/keymaps/` binds the
/// workspace-level commands (ToggleSidebar, ToggleConsole, …) against it.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

/// The window shell (PRD §3): docks at the left and bottom edges, the center pane
/// between them, and the status bar along the bottom.
pub struct Workspace {
    focus_handle: FocusHandle,
    left_dock: Entity<Dock>,
    bottom_dock: Entity<Dock>,
    center_pane: Entity<Pane>,
    status_bar: Entity<StatusBar>,
}

impl Workspace {
    /// A fresh workspace: both docks open (REQ-UI-01/04), an empty center pane and an
    /// empty status bar waiting for `add_left_item` / `add_right_item`.
    ///
    /// Contract: a new [`Workspace`] takes keyboard focus in its window, so the shell
    /// shortcuts (`ToggleSidebar`, `ToggleConsole`, …) work before any click.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            focus_handle,
            left_dock: cx.new(|cx| Dock::new(DockPosition::Left, cx)),
            bottom_dock: cx.new(|cx| Dock::new(DockPosition::Bottom, cx)),
            center_pane: cx.new(Pane::new),
            status_bar: cx.new(StatusBar::new),
        }
    }

    /// The left dock: collections, environments and history (REQ-UI-01).
    pub fn left_dock(&self) -> &Entity<Dock> {
        &self.left_dock
    }

    /// The bottom dock: the console (REQ-UI-04).
    pub fn bottom_dock(&self) -> &Entity<Dock> {
        &self.bottom_dock
    }

    /// The center pane of open request items (REQ-UI-02).
    pub fn center_pane(&self) -> &Entity<Pane> {
        &self.center_pane
    }

    /// The status bar along the bottom edge.
    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    /// Show or hide the left dock (REQ-UI-01); the bottom dock is untouched.
    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.left_dock.update(cx, |dock, cx| dock.toggle(cx));
        cx.notify();
    }

    /// Show or hide the console in the bottom dock (REQ-UI-04); the left dock is untouched.
    fn toggle_console(&mut self, _: &ToggleConsole, _window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_dock.update(cx, |dock, cx| dock.toggle(cx));
        cx.notify();
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("workspace")
            .size_full()
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_console))
            .bg(cx.theme().colors().background)
            // The docks and the pane share the rows above the console.
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .children(
                        self.left_dock
                            .read(cx)
                            .is_open()
                            .then(|| self.left_dock.clone()),
                    )
                    .child(self.center_pane.clone()),
            )
            .children(
                self.bottom_dock
                    .read(cx)
                    .is_open()
                    .then(|| self.bottom_dock.clone()),
            )
            .child(self.status_bar.clone())
    }
}

/// Wire the workspace crate into the app's init order.
///
/// Decision: [`ToggleSidebar`] and [`ToggleConsole`] are answered by element-level
/// `.on_action` listeners on the [`Workspace`] root ([`WORKSPACE_CONTEXT`]), which is
/// where GPUI delivers actions — there is no global action registry to register into and
/// no keys to bind here (`pustman_actions` owns the keymaps). The hook stays because the
/// init order calls every UI crate's `init`, and panel registration (a later step) grows
/// it.
pub fn init(_cx: &mut App) {}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
