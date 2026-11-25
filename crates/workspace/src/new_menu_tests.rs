//! The "New" menu on the pane's "+" button (REQ-UI-03): it lists every request kind, an
//! entry whose kind has no editor yet is shown but cannot run, and choosing one dispatches
//! that kind's action — the same action the command palette runs. The menu is driven the
//! way a user drives it: a click on "+", then a click on an entry.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Context, Entity, IntoElement, Modifiers, ParentElement, Render, TestAppContext,
    VisualTestContext, Window,
};
use pustman_actions::workspace::NewRequest;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::new_menu::{NEW_MENU_BUTTON, NewMenu};

/// Every kind the menu offers, by its entry label (MQTT waits for M6).
const KIND_LABELS: [&str; 5] = ["HTTP", "GraphQL", "WebSocket", "Socket.IO", "gRPC"];

/// The window root: the "+" button the way the pane's tab strip draws it.
struct Host {
    new_menu: Entity<NewMenu>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.new_menu.clone())
    }
}

/// A window holding the "+" button, on the app's real init order.
fn host_window(cx: &mut TestAppContext) -> (Entity<NewMenu>, &mut VisualTestContext) {
    cx.update(init_test);
    let (host, cx) = cx.add_window_view(|_window, cx| Host {
        new_menu: cx.new(NewMenu::new),
    });
    let new_menu = cx.read(|cx| host.read(cx).new_menu.clone());
    draw_window(cx);
    (new_menu, cx)
}

/// Click the element drawn under `selector`, then paint the result.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = match cx.debug_bounds(selector) {
        Some(bounds) => bounds,
        None => panic!("{selector} must be on screen"),
    };
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn is_open(new_menu: &Entity<NewMenu>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| new_menu.read(cx).is_open())
}

#[gpui::test]
fn new_menu_lists_every_request_kind(cx: &mut TestAppContext) {
    let (new_menu, cx) = host_window(cx);
    assert!(!is_open(&new_menu, cx), "the menu starts closed");

    click(cx, NEW_MENU_BUTTON);

    assert!(is_open(&new_menu, cx), "\"+\" opens the New menu");
    for label in KIND_LABELS {
        assert!(
            cx.debug_bounds(label).is_some(),
            "the New menu lists {label}"
        );
    }
}

#[gpui::test]
fn kind_without_editor_is_disabled(cx: &mut TestAppContext) {
    let (new_menu, cx) = host_window(cx);
    // HTTP has an editor (the request editor answers `NewRequest`); GraphQL has none yet.
    cx.update(|_, cx| {
        cx.on_action(|_: &NewRequest, _cx| {});
    });

    click(cx, NEW_MENU_BUTTON);

    // An entry's hint sits under its label plus `-hint` (`ui::context_menu::HINT_SUFFIX`).
    assert!(
        cx.debug_bounds("GraphQL-hint").is_some(),
        "a kind with no editor says it is not available yet"
    );
    assert!(
        cx.debug_bounds("HTTP-hint").is_none(),
        "a kind with an editor carries no such note"
    );

    click(cx, "GraphQL");
    assert!(
        is_open(&new_menu, cx),
        "choosing a kind with no editor runs nothing and leaves the menu open"
    );
}

#[gpui::test]
fn choosing_http_dispatches_new_request(cx: &mut TestAppContext) {
    let (new_menu, cx) = host_window(cx);
    let created = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let created = created.clone();
        cx.on_action(move |_: &NewRequest, _cx| created.set(created.get() + 1));
    });

    click(cx, NEW_MENU_BUTTON);
    click(cx, "HTTP");

    assert_eq!(created.get(), 1, "choosing HTTP dispatches NewRequest once");
    assert!(
        !is_open(&new_menu, cx),
        "the menu closes once an entry runs"
    );
}
