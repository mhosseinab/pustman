//! The "New" menu on the pane's "+" button (REQ-UI-03): one entry per request kind —
//! HTTP, GraphQL, WebSocket, Socket.IO and gRPC. Each entry dispatches its
//! `pustman_actions::workspace` action, the same one the command palette runs, so the menu
//! and the palette can never disagree about what a kind opens. A kind whose editor has
//! not landed yet answers no action; its entry is shown but cannot run, and says so.
//!
//! `Ctrl/Cmd-T` (`NewTab`) does not go through this menu: it still opens HTTP directly.
//!
//! Decision: the step's "Not available yet" tooltip is a visible inline note at the end of
//! the disabled entry (the menu's hint) — `ui::ContextMenu` has no per-entry tooltip, and a
//! note is readable from the keyboard, where a hover tooltip never appears.

use gpui::{Action, Context, Entity, FocusHandle, IntoElement, Render, Subscription, Window};
use pustman_actions::workspace::{
    NewGraphQlRequest, NewGrpcRequest, NewRequest, NewSocketIoRequest, NewTab, NewWebSocketRequest,
};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;
use ui::{IconName, Tooltip};

/// Test hook: the "+" button the menu opens from.
pub const NEW_MENU_BUTTON: &str = "new-menu-button";

/// The height of the tab strip the button sits in — the redesign pack's
/// `layout.requestTabs`, 40 px — and so how far below the button the menu drops.
const TRIGGER_HEIGHT_PX: f32 = 40.;

/// What a kind's entry says while no editor answers its action.
const NOT_AVAILABLE: &str = "Not available yet";

/// Builds the action a kind's entry dispatches, fresh for each menu.
type KindAction = fn() -> Box<dyn Action>;

/// The request kinds, in menu order: each entry's label and the action it dispatches.
/// MQTT is left out until M6 gives it a client.
const REQUEST_KINDS: [(&str, KindAction); 5] = [
    ("HTTP", || Box::new(NewRequest)),
    ("GraphQL", || Box::new(NewGraphQlRequest)),
    ("WebSocket", || Box::new(NewWebSocketRequest)),
    ("Socket.IO", || Box::new(NewSocketIoRequest)),
    ("gRPC", || Box::new(NewGrpcRequest)),
];

/// The "+" button at the end of the pane's tab strip and the "New" menu it opens.
pub struct NewMenu {
    focus_handle: FocusHandle,
    menu: Option<Entity<ContextMenu>>,
    _menu_subscription: Option<Subscription>,
}

impl NewMenu {
    /// A closed menu behind its "+" button.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            menu: None,
            _menu_subscription: None,
        }
    }

    /// Whether the menu is showing.
    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    /// Open the menu under the "+" button and give it the keyboard. Which kinds can run
    /// is read now, from the actions available where the button sits: an editor that
    /// lands later enables its entry the next time the menu opens.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open() {
            return;
        }
        let menu = cx.new(|cx| {
            REQUEST_KINDS
                .iter()
                .fold(ContextMenu::new(cx), |menu, (label, action)| {
                    let action = action();
                    // The window's own dispatch path covers handlers on the views around
                    // the button; the app's covers global ones (`request_editor`'s
                    // `NewRequest`).
                    let available = window.is_action_available(action.as_ref(), cx)
                        || cx.is_action_available(action.as_ref());
                    if available {
                        menu.entry(*label, move |_event, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx);
                        })
                    } else {
                        menu.disabled_entry(*label).hint(NOT_AVAILABLE)
                    }
                })
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.menu = Some(menu);
        self._menu_subscription = Some(subscription);
        cx.notify();
    }

    /// The menu is done — an entry ran or Escape backed out — so it goes away and the
    /// keyboard returns to the "+" button.
    fn on_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.menu = None;
        self._menu_subscription = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }
}

impl Render for NewMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let trigger_height = px_to_rems(TRIGGER_HEIGHT_PX);
        h_flex()
            .relative()
            .flex_none()
            .h(trigger_height)
            .px(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .debug_selector(|| NEW_MENU_BUTTON.to_owned())
                    .flex_none()
                    .child(
                        // The tooltip teaches `Ctrl/Cmd-T` — the HTTP shortcut — and hides
                        // while the menu is open so it cannot paint over the entries (#265).
                        IconButton::new(
                            NEW_MENU_BUTTON,
                            IconName::Plus,
                            Tooltip::for_action("New…", &NewTab).hidden(self.is_open()),
                        )
                        .focus_handle(self.focus_handle.clone())
                        .on_click(cx.listener(|this, _event, window, cx| this.open(window, cx))),
                    ),
            )
            .children(floating_menu(&self.menu, trigger_height, cx))
    }
}

#[cfg(test)]
#[path = "new_menu_tests.rs"]
mod tests;
