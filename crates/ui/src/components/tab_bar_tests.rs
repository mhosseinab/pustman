//! The tab bar's single activation path: clicking a tab and confirming with Enter run the
//! same [`TabBar`] code (`components/behavior.rs`), so the mouse and the keyboard can
//! never disagree — and a disabled tab is inert, down to having no close button.

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};

use crate::components::behavior::Disableable;
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

#[gpui::test]
fn clicking_a_tab_activates_it_exactly_once(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (bar, cx) = bar_with(
        cx,
        vec![Tab::new("first", "First"), Tab::new("second", "Second")],
    );
    let mut events = cx.events(&bar);
    draw_window(cx);

    let second = cx.debug_bounds("second").expect("the second tab rendered");
    cx.simulate_click(second.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        drain(&mut events),
        vec![TabBarEvent::Activated { index: 1 }]
    );
}

#[gpui::test]
fn enter_on_the_focused_bar_activates_the_selected_tab_exactly_once(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (bar, cx) = bar_with(
        cx,
        vec![Tab::new("first", "First"), Tab::new("second", "Second")],
    );
    let mut events = cx.events(&bar);
    draw_window(cx);

    let focus_handle = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    // The bar's key handlers register when the focused frame paints.
    draw_window(cx);

    press_key(cx, "right");
    assert_eq!(drain(&mut events), vec![TabBarEvent::Selected { index: 1 }]);

    press_key(cx, "enter");
    assert_eq!(
        drain(&mut events),
        vec![TabBarEvent::Activated { index: 1 }]
    );
}

#[gpui::test]
fn a_disabled_tab_has_no_close_button_and_never_activates(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (bar, cx) = bar_with(
        cx,
        vec![
            Tab::new("off", "Off").disabled(true).on_close(|_, _, _| {}),
            Tab::new("on", "On").on_close(|_, _, _| {}),
        ],
    );
    let mut events = cx.events(&bar);
    draw_window(cx);

    // The enabled tab beside it shows the control the disabled one leaves out — so these
    // two assertions cannot pass just because the selectors failed to record.
    assert!(
        cx.debug_bounds("on-close").is_some(),
        "the enabled tab shows its close button"
    );
    assert!(
        cx.debug_bounds("off-close").is_none(),
        "the disabled tab shows none"
    );

    let off = cx.debug_bounds("off").expect("the disabled tab rendered");
    cx.simulate_click(off.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        drain(&mut events),
        Vec::new(),
        "clicking the disabled tab activates nothing"
    );

    // It is the selected tab (index 0), so Enter on the focused bar would activate it —
    // if a disabled tab could be activated.
    let focus_handle = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    assert_eq!(
        drain(&mut events),
        Vec::new(),
        "Enter never activates the disabled tab"
    );
}
