//! A tab's hover tooltip earns its place only when it says something the tab does not
//! already show (#231): a clean, untruncated tab's label is its own tooltip, so a balloon
//! repeating it only covers the bar below. A dirty tab's "unsaved changes" is new
//! information and stays.

use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};

use crate::components::tab::Tab;
use crate::components::tab_bar::TabBar;
use crate::components::tooltip::TOOLTIP_SELECTOR;
use crate::test_support::{draw_window, init_test};

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// Rest the pointer on the tab `selector` until a tooltip would show; report whether one did.
fn tooltip_after_hovering(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    draw_window(cx);
    let tab = cx.debug_bounds(selector).expect("the tab rendered");
    cx.simulate_mouse_move(tab.center(), None, gpui::Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    cx.debug_bounds(TOOLTIP_SELECTOR).is_some()
}

#[gpui::test]
fn test_a_dirty_tab_explains_its_dot_on_hover(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        TabBar::new(cx).with_tabs(vec![Tab::new("draft", "Draft").dirty(true)])
    });
    assert!(
        tooltip_after_hovering(cx, "draft"),
        "the dirty tab's tooltip says it has unsaved changes"
    );
}

#[gpui::test]
fn test_a_clean_tab_does_not_repeat_its_label_in_a_tooltip(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) =
        cx.add_window_view(|_, cx| TabBar::new(cx).with_tabs(vec![Tab::new("params", "Params")]));
    assert!(
        !tooltip_after_hovering(cx, "params"),
        "a clean tab's label is fully visible, so a tooltip would only repeat it"
    );
}

#[gpui::test]
fn test_a_tab_tooltip_opens_below_the_pointer_not_over_the_tab(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (_bar, cx) = cx.add_window_view(|_, cx| {
        TabBar::new(cx).with_tabs(vec![Tab::new("draft", "Draft").dirty(true)])
    });
    assert!(
        tooltip_after_hovering(cx, "draft"),
        "the dirty tab shows a tooltip"
    );
    let tab = cx.debug_bounds("draft").expect("the tab rendered");
    let tooltip = cx
        .debug_bounds(TOOLTIP_SELECTOR)
        .expect("the tooltip rendered");
    assert!(
        tooltip.top() >= tab.bottom(),
        "the tooltip ({tooltip:?}) is drawn over the tab it describes ({tab:?})"
    );
}
