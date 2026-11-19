//! The [`Welcome`] item: the pane's empty state and the tab `NewTab` opens until the
//! request editor lands. Real UI, not a placeholder: what the workbench is for, plus the
//! two shortcuts a first-time user needs next (REQ-UI-02).

use gpui::{Action, App, Context, IntoElement, ParentElement, Render, SharedString, Window};
use pustman_actions::workspace::{NewTab, ToggleSidebar};
use ui::prelude::*;

use crate::item::Item;

/// The welcome view's stable id and test selector.
const WELCOME_ID: &str = "welcome";

/// The pane's empty state: a heading, one line about the workbench, and the shortcut
/// hints for opening a tab and getting the sidebar back.
pub struct Welcome;

impl Welcome {
    /// A fresh welcome item.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Item for Welcome {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Welcome")
    }
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(WELCOME_ID)
            .debug_selector(|| WELCOME_ID.to_owned())
            .size_full()
            .justify_center()
            .items_center()
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(Headline::new("Welcome to Pustman"))
            .child(
                Label::new("Send, inspect and organize HTTP requests — start with a new tab.")
                    .color(Color::Muted),
            )
            .child(
                v_flex()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(shortcut_hint("New Tab", &NewTab, cx))
                    .child(shortcut_hint("Toggle Sidebar", &ToggleSidebar, cx)),
            )
    }
}

/// One hint row: what the shortcut does, and the keys that run it — read from the active
/// keymap at render time, so a rebound key cannot go stale here.
fn shortcut_hint(label: &'static str, action: &dyn Action, cx: &App) -> impl IntoElement {
    h_flex()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(Label::new(label).color(Color::Muted))
        .child(KeyBinding::for_action(action))
}
