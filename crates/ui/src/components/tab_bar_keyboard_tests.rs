//! The tab bar cycles its selection with the keyboard: Right walks 0 → 1 → 2 and wraps
//! back to 0, Left wraps the other way. One bar, one selection ring — no arrow press ever
//! leaves the selection out of reach of the next one.

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Entity, Focusable, TestAppContext, VisualTestContext};

use crate::components::tab::Tab;
use crate::components::tab_bar::{TabBar, TabBarEvent};
use crate::test_support::{draw_window, init_test, press_key};

/// A bar of `tabs` in its own window, ready to draw.
fn bar_with(cx: &mut TestAppContext, tabs: Vec<Tab>) -> (Entity<TabBar>, &mut VisualTestContext) {
    cx.add_window_view(|_, cx| TabBar::new(cx).with_tabs(tabs))
}

/// Every [`TabBarEvent`] the bar has emitted since the last call.
fn drain(events: &mut UnboundedReceiver<TabBarEvent>) -> Vec<TabBarEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

/// The index the bar has selected right now.
fn selected(cx: &mut VisualTestContext, bar: &Entity<TabBar>) -> usize {
    cx.read(|cx| bar.read_with(cx, |bar, _| bar.selected()))
}

#[gpui::test]
fn tab_bar_cycles_with_keyboard(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (bar, cx) = bar_with(
        cx,
        vec![
            Tab::new("first", "First"),
            Tab::new("second", "Second"),
            Tab::new("third", "Third"),
        ],
    );
    let mut events = cx.events(&bar);
    draw_window(cx);

    let focus_handle = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    // The bar's key handlers register when the focused frame paints.
    draw_window(cx);
    assert_eq!(selected(cx, &bar), 0, "the bar starts on the first tab");
    assert_eq!(drain(&mut events), Vec::new(), "focusing selects nothing");

    // Right walks the ring and wraps back to the start.
    for expected in [1, 2, 0] {
        press_key(cx, "right");
        assert_eq!(
            selected(cx, &bar),
            expected,
            "Right moves the selection one tab on, wrapping at the end"
        );
        assert_eq!(
            drain(&mut events),
            vec![TabBarEvent::Selected { index: expected }],
            "each move announces exactly one Selected event for the index reached"
        );
    }

    // Left wraps back around the other way.
    press_key(cx, "left");
    assert_eq!(
        selected(cx, &bar),
        2,
        "Left from the first tab wraps to the last"
    );
    assert_eq!(
        drain(&mut events),
        vec![TabBarEvent::Selected { index: 2 }],
        "the wrap announces the index it landed on"
    );
}
