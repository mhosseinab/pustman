//! The [`Welcome`] item: the pane's empty state, and the tab `NewTab` opens when no crate
//! answers `NewRequest`. Real UI, not a placeholder: what the workbench is for, a button
//! that starts a request, and the sidebar shortcut (REQ-UI-02).

use gpui::{
    Action, App, Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    Window,
};
use pustman_actions::workspace::{NewTab, ToggleSidebar};
use ui::prelude::*;

use crate::item::Item;

/// The welcome view's stable id and test selector.
const WELCOME_ID: &str = "welcome";

/// The New Request button's stable id and test selector.
const NEW_REQUEST_ID: &str = "welcome-new-request";

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
                Label::new("Send, inspect and organize HTTP requests — start with a new request.")
                    .color(Color::Muted),
            )
            .child(
                v_flex()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    // The shortcut sits beside the button, not inside it: a key chip on
                    // the filled accent background is unreadable.
                    .child(
                        h_flex()
                            .gap(DynamicSpacing::Base04.rems(cx))
                            .child(
                                div().debug_selector(|| NEW_REQUEST_ID.to_owned()).child(
                                    Button::new(NEW_REQUEST_ID, "New Request")
                                        .style(ButtonStyle::Filled)
                                        .start_icon(Icon::new(IconName::Plus))
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(NewTab), cx)
                                        }),
                                ),
                            )
                            .child(KeyBinding::for_action(&NewTab)),
                    )
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
