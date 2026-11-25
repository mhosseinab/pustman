//! The split Send (mockup section 1): the accent "Send" and, joined to it, a chevron
//! that opens a small menu of the editor's existing actions — Send and Copy as cURL.
//! Each entry dispatches its `request::` action on the editor's own focus handle, so it
//! runs exactly the handler the keyboard reaches. While a request is in flight the
//! button is "Cancel" (`send`) and the chevron steps aside: its entries would act on a
//! request that is already going out.

use gpui::{AnyElement, Context, Entity, Subscription, Window};
use pustman_actions::request;
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;

use crate::RequestEditor;

/// Test hook: the chevron half of the split Send.
pub const SEND_MENU_SELECTOR: &str = "send-menu-chevron";

/// The height of the Send row (a [`ButtonSize::Large`]): the menu drops below it.
const SEND_ROW_HEIGHT_PX: f32 = 32.;

/// The open chevron menu and the subscription that drops it when it is done.
pub(crate) struct SendMenu {
    menu: Entity<ContextMenu>,
    _subscription: Subscription,
}

impl RequestEditor {
    /// Open the chevron's menu under it, with the keyboard in it.
    pub(crate) fn open_send_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.send_menu.is_some() {
            return;
        }
        let send_target = self.focus_handle.clone();
        let curl_target = self.focus_handle.clone();
        let menu = cx.new(|cx| {
            ContextMenu::new(cx)
                .entry(SharedString::new_static("Send"), move |_event, window, cx| {
                    send_target.dispatch_action(&request::Send, window, cx)
                })
                .entry(
                    SharedString::new_static("Copy as cURL"),
                    move |_event, window, cx| {
                        curl_target.dispatch_action(&request::CopyAsCurl, window, cx)
                    },
                )
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_send_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.send_menu = Some(SendMenu {
            menu,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// The menu is done — an entry ran or Escape backed out — so it goes, and the
    /// keyboard returns to the editor.
    fn on_send_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.send_menu = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The chevron half of the split Send, with its menu floating under it. `None`
    /// while a request is in flight.
    pub(crate) fn render_send_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.is_sending() {
            return None;
        }
        let open = self.send_menu.as_ref().map(|menu| menu.menu.clone());
        Some(
            div()
                .debug_selector(|| SEND_MENU_SELECTOR.to_string())
                .relative()
                .child(
                    IconButton::new(
                        "send-menu-chevron",
                        IconName::ChevronDown,
                        Tooltip::text("More ways to send").hidden(open.is_some()),
                    )
                    .style(ButtonStyle::Filled)
                    .size(ButtonSize::Large)
                    .on_click(cx.listener(|editor, _event, window, cx| {
                        editor.open_send_menu(window, cx)
                    })),
                )
                .children(floating_menu(&open, px_to_rems(SEND_ROW_HEIGHT_PX), cx))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "send_menu_tests.rs"]
mod tests;
