//! The pane tab strip's chrome (#214): thin separators delimit the tabs the strip does
//! not highlight, and a tab's trailing slot shows one glyph at a time — the dirty dot at
//! rest, the close × once the tab is hovered. Only one of the two is ever offered to the
//! pointer, so a click can never find a × the strip is not showing.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

use crate::components::behavior::{Disableable, Toggleable};
use crate::components::tab::Tab;
use crate::components::tab_bar::TabBar;
use crate::test_support::{draw_window, init_test};

/// A bar of `tabs` in its own window, ready to draw.
fn bar_with(cx: &mut TestAppContext, tabs: Vec<Tab>) -> (Entity<TabBar>, &mut VisualTestContext) {
    cx.add_window_view(|_, cx| TabBar::new(cx).with_tabs(tabs))
}

/// `debug_bounds` takes a `&'static str`, so the built string is leaked — fine for a
/// test's handful of selectors.
fn close_selector(tab: &str) -> &'static str {
    format!("{tab}-close").leak()
}

/// The selector of the dirty dot on the tab `tab` (see [`close_selector`]).
fn dot_selector(tab: &str) -> &'static str {
    format!("{tab}-dot").leak()
}

/// A dirty tab whose close button counts the calls it gets.
fn dirty_tab(id: &'static str, label: &'static str, closes: &Rc<Cell<usize>>) -> Tab {
    let closes = closes.clone();
    Tab::new(id, label)
        .dirty(true)
        .on_close(move |_, _, _| closes.set(closes.get() + 1))
}

#[gpui::test]
fn test_the_tab_strip_separates_the_tabs_it_does_not_highlight(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (bar, cx) = bar_with(
        cx,
        vec![
            Tab::new("first", "First"),
            Tab::new("second", "Second"),
            Tab::new("third", "Third"),
            Tab::new("fourth", "Fourth"),
        ],
    );
    draw_window(cx);

    // The first tab is selected: its own edges delimit it, so a separator beside it
    // would only repeat the highlight, and the strip starts without one.
    assert!(
        cx.debug_bounds("first-separator").is_none(),
        "the strip never draws a separator in front of its first tab"
    );
    assert!(
        cx.debug_bounds("second-separator").is_none(),
        "a separator beside the active tab would only repeat its edge"
    );
    assert!(
        cx.debug_bounds("third-separator").is_some(),
        "the two untouched tabs are separated from each other"
    );
    assert!(
        cx.debug_bounds("fourth-separator").is_some(),
        "every boundary between two tabs the strip does not highlight is marked"
    );

    cx.update(|_, cx| bar.update(cx, |bar, cx| bar.set_selected(1, cx)));
    draw_window(cx);
    assert!(
        cx.debug_bounds("second-separator").is_none(),
        "the separator beside the newly active tab is gone"
    );
    assert!(
        cx.debug_bounds("third-separator").is_none(),
        "the separator beside the newly active tab is gone"
    );
    assert!(
        cx.debug_bounds("fourth-separator").is_some(),
        "the one boundary the highlight does not touch keeps its separator"
    );
}

#[gpui::test]
fn test_the_dirty_dot_replaces_the_close_button_until_the_tab_is_hovered(cx: &mut TestAppContext) {
    cx.update(init_test);
    let closes = Rc::new(Cell::new(0usize));
    let (_bar, cx) = bar_with(
        cx,
        vec![
            // The selected tab and an inactive one: the dot owns the slot in both.
            dirty_tab("draft", "Draft", &closes),
            dirty_tab("spare", "Spare", &closes),
        ],
    );
    draw_window(cx);

    let draft_close = cx
        .debug_bounds(close_selector("draft"))
        .expect("the dirty tab lays out its close slot");
    let spare_close = cx
        .debug_bounds(close_selector("spare"))
        .expect("the inactive dirty tab lays out its close slot");
    assert!(
        cx.debug_bounds(dot_selector("draft")).is_some(),
        "the dirty tab shows its dot"
    );

    // A GPUI test window parks its pointer at (0, 0) — on the first tab. Move it off the
    // strip to see the tabs the way a resting pointer does.
    cx.simulate_mouse_move(point(px(10_000.), px(10_000.)), None, Modifiers::none());
    draw_window(cx);

    // At rest the dot has replaced the ×, so the slot offers the pointer nothing.
    cx.simulate_click(draft_close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        closes.get(),
        0,
        "the active dirty tab offers its dot at rest — clicking where the × hides must not close"
    );
    cx.simulate_click(spare_close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        closes.get(),
        0,
        "an inactive dirty tab offers its dot at rest too"
    );

    // Hovering the tab swaps the dot for the ×.
    let draft = cx.debug_bounds("draft").expect("the tab rendered");
    cx.simulate_mouse_move(draft.center(), None, Modifiers::none());
    draw_window(cx);
    let draft_close = cx
        .debug_bounds(close_selector("draft"))
        .expect("the close slot kept its place");
    cx.simulate_click(draft_close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        closes.get(),
        1,
        "the close button replaced the dot while the tab was hovered"
    );

    let spare = cx.debug_bounds("spare").expect("the tab rendered");
    cx.simulate_mouse_move(spare.center(), None, Modifiers::none());
    draw_window(cx);
    let spare_close = cx
        .debug_bounds(close_selector("spare"))
        .expect("the close slot kept its place");
    cx.simulate_click(spare_close.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        closes.get(),
        2,
        "hovering reveals the × on the inactive tab as well"
    );
}

#[gpui::test]
fn test_a_tab_that_cannot_be_closed_keeps_showing_its_dirty_dot(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) = bar_with(
        cx,
        vec![
            Tab::new("draft", "Draft")
                .dirty(true)
                .disabled(true)
                .toggle_state(true),
        ],
    );
    draw_window(cx);

    assert!(
        cx.debug_bounds(dot_selector("draft")).is_some(),
        "nothing can replace the dot on a tab with no close button, so it stays"
    );
    assert!(
        cx.debug_bounds(close_selector("draft")).is_none(),
        "a disabled tab renders no close button at all"
    );
}
