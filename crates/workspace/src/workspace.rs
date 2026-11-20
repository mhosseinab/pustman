//! `workspace` — the window shell every Pustman screen lives in (PRD §3): the
//! [`Workspace`] entity with its left [`Dock`] (collections, environments, history), the
//! center [`Pane`] of request items, the bottom [`Dock`] (console) and the [`StatusBar`]
//! (`add_left_item` / `add_right_item`). Zed's `workspace` crate is GPL; these are our own
//! implementations of the shapes in
//! `.agents/skills/rust-gpui-ui-ux/references/gpui-fundamentals.md`, and every color comes
//! from `cx.theme()` so light and dark stay correct.

mod autosave;
mod dock;
mod item;
mod modal_layer;
mod pane;
mod pane_tabs;
mod panel;
mod restore;
mod status_bar;
mod title_bar;
mod toast_layer;
mod welcome;

pub use autosave::Autosave;
pub use dock::{Dock, DockPosition};
pub use item::{AnyViewItem, Item, ItemHandle};
pub use modal_layer::{ModalLayer, ModalView};
pub use pane::Pane;
pub use panel::{Panel, PanelHandle};
pub use restore::{ItemFactory, Restore};
pub use status_bar::StatusBar;
pub use title_bar::TitleBar;
pub use toast_layer::{TOAST_DURATION, ToastAction, ToastHandler};
pub use welcome::Welcome;

use gpui::{
    App, Context, Decorations, Entity, FocusHandle, Focusable, ParentElement, Render, Window,
};
use pustman_actions::workspace::{
    CloseTab, NewTab, NextTab, PreviousTab, ToggleConsole, ToggleSidebar,
};
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
    title_bar: Entity<TitleBar>,
    toasts: Entity<ui::Toasts>,
    modal_layer: ModalLayer,
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
            title_bar: cx.new(TitleBar::new),
            toasts: cx.new(Toasts::new),
            modal_layer: ModalLayer::default(),
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

    /// Open a new tab in the center pane (REQ-UI-02).
    fn new_tab(&mut self, _: &NewTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.center_pane.update(cx, |pane, cx| pane.new_tab(cx));
    }

    /// Close the active tab in the center pane (REQ-UI-02).
    fn close_tab(&mut self, _: &CloseTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.center_pane
            .update(cx, |pane, cx| pane.close_active_tab(cx));
    }

    /// Activate the next tab in the center pane (REQ-UI-02).
    fn next_tab(&mut self, _: &NextTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.center_pane
            .update(cx, |pane, cx| pane.activate_next_tab(cx));
    }

    /// Activate the previous tab in the center pane (REQ-UI-02).
    fn previous_tab(&mut self, _: &PreviousTab, _window: &mut Window, cx: &mut Context<Self>) {
        self.center_pane
            .update(cx, |pane, cx| pane.activate_previous_tab(cx));
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let decorations = window.window_decorations();
        let shell = v_flex()
            .id("workspace")
            .size_full()
            // The two overlays at the end position themselves against the root.
            .relative()
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_console))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .bg(cx.theme().colors().background)
            // Only a client-decorated window draws its own title bar; the compositor draws
            // one around a server-decorated window.
            .when(matches!(decorations, Decorations::Client { .. }), |shell| {
                shell.child(self.title_bar.clone())
            })
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
            // Toasts float bottom-right above the status bar; the modal layer covers all.
            .child(self.toasts_overlay(cx))
            .child(self.modal_overlay(cx));
        match decorations {
            Decorations::Server => shell.into_any_element(),
            Decorations::Client { tiling } => title_bar::client_frame(tiling, shell, window, cx),
        }
    }
}

/// Wire the workspace crate into the app's init order.
///
/// Decision: [`ToggleSidebar`] and [`ToggleConsole`] are answered by element-level
/// `.on_action` listeners on the [`Workspace`] root ([`WORKSPACE_CONTEXT`]), which is
/// where GPUI delivers actions — there is no global action registry to register into and
/// no keys to bind here (`pustman_actions` owns the keymaps). The tab actions
/// (`NewTab`/`CloseTab`/`NextTab`/`PreviousTab`) dispatch the same way: the listeners on
/// the root delegate one line each to the `Pane` methods in `pane_tabs`. The hook stays
/// because the init order calls every UI crate's `init`, and panel registration (a later
/// step) grows it.
pub fn init(_cx: &mut App) {}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
