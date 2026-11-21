//! The method button's hover tooltip stays out of its own dropdown (#265): with the
//! pointer still on the anchor after the menu opens, the balloon paints over the rows
//! being read. A control's tooltip hides while its own popover is open, and comes back
//! when the popover is gone.

use std::time::Duration;

use api_model::Method;
use gpui::{Modifiers, Pixels, Point, TestAppContext, VisualTestContext};
use ui::components::tooltip::TOOLTIP_SELECTOR;
use ui::test_support::{draw_window, init_test, press_key};

use crate::url_bar::{METHOD_CONTROL_SELECTOR, UrlBar};

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// A dropdown entry's debug selector: proof the method menu is open (its entries are
/// keyed by their labels).
const MENU_ENTRY_SELECTOR: &str = "GET";

fn url_bar_window(cx: &mut TestAppContext) -> (gpui::Entity<UrlBar>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        // The dropdown's Escape path needs the menu keys bound.
        text_input::init(cx);
        crate::init(cx);
    });
    let (bar, cx) =
        cx.add_window_view(|window, cx| UrlBar::new(Method::Get, String::new(), window, cx));
    draw_window(cx);
    (bar, cx)
}

/// Rest the pointer at `point` until a hover tooltip would show; report whether one did.
/// Moving the pointer is what re-triggers the tooltip machinery — the same as a user
/// nudging the mouse while it rests on the control.
fn tooltip_after_hovering(cx: &mut VisualTestContext, point: Point<Pixels>) -> bool {
    cx.simulate_mouse_move(point, None, Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    cx.debug_bounds(TOOLTIP_SELECTOR).is_some()
}

#[gpui::test]
fn test_the_method_tooltip_stays_out_of_its_own_menu(cx: &mut TestAppContext) {
    let (_bar, cx) = url_bar_window(cx);
    let method = cx
        .debug_bounds(METHOD_CONTROL_SELECTOR)
        .expect("the method control rendered");
    assert!(
        tooltip_after_hovering(cx, method.center()),
        "the method button explains itself on hover"
    );

    cx.simulate_click(method.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(MENU_ENTRY_SELECTOR).is_some(),
        "clicking the method button opens its dropdown"
    );

    // The pointer rests on the anchor while the menu is open — the #265 shot.
    assert!(
        !tooltip_after_hovering(cx, method.center()),
        "the tooltip hides while its own menu is open"
    );
}

#[gpui::test]
fn test_the_method_tooltip_returns_when_its_menu_closes(cx: &mut TestAppContext) {
    let (_bar, cx) = url_bar_window(cx);
    let method = cx
        .debug_bounds(METHOD_CONTROL_SELECTOR)
        .expect("the method control rendered");

    cx.simulate_click(method.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(MENU_ENTRY_SELECTOR).is_none(),
        "escape dismissed the menu"
    );

    assert!(
        tooltip_after_hovering(cx, method.center()),
        "the tooltip comes back once the menu is gone — it is hidden, not dropped"
    );
}
