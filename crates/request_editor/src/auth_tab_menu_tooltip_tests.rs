//! The Auth tab picker triggers' hover tooltips stay out of their own dropdowns
//! (#267): with the pointer still on a trigger after its menu opens, the balloon
//! paints over the rows being read. A trigger's tooltip hides while its own menu is
//! open, and comes back when the menu is gone — the `url_bar` method dropdown's
//! behavior (#265) everywhere a picker trigger lives.

use std::time::Duration;

use api_model::{ApiKeyLocation, Auth, HttpRequest};
use gpui::{Bounds, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point, px};
use ui::components::tooltip::TOOLTIP_SELECTOR;
use ui::test_support::{draw_window, init_test, press_key};

use crate::RequestEditor;

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// The type dropdown trigger, wrapped by its form row.
const TYPE_CONTROL_SELECTOR: &str = "auth-control-type";

/// The API key location dropdown trigger, likewise.
const LOCATION_CONTROL_SELECTOR: &str = "auth-control-location";

/// A dropdown entry's debug selector: proof the type menu is open (entries are keyed
/// by their labels).
const TYPE_MENU_ENTRY_SELECTOR: &str = "API key";

/// Proof the location menu is open.
const LOCATION_MENU_ENTRY_SELECTOR: &str = "Query parameter";

/// A request whose Auth tab shows both picker triggers at once: the type dropdown
/// and — under API key — the location dropdown.
fn api_key_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.auth = Auth::ApiKey {
        key: None,
        value: None,
        location: Some(ApiKeyLocation::Header),
        extra: Default::default(),
    };
    request
}

/// The Auth tab on screen, opened the way a user opens it: click the tab in the
/// request tab strip.
fn open_auth_tab(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(|cx| {
        // The dropdown's Escape path needs the menu keys bound.
        text_input::init(cx);
        crate::init(cx);
    });
    let (_editor, cx) =
        cx.add_window_view(|window, cx| RequestEditor::new(api_key_request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    let tab = cx
        .debug_bounds("request-tab-auth")
        .expect("the Auth tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    cx
}

fn control_bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

/// A point on the dropdown trigger itself. The trigger is the first thing in its
/// control column, so the column's left edge is the trigger's left edge and the
/// column's vertical middle is the trigger's own middle.
fn trigger_point(control: Bounds<Pixels>) -> Point<Pixels> {
    point(control.left() + px(12.), control.center().y)
}

/// Rest the pointer at `point` until a hover tooltip would show; report whether one
/// did. Moving the pointer is what re-triggers the tooltip machinery — the same as a
/// user nudging the mouse while it rests on the control.
fn tooltip_after_hovering(cx: &mut VisualTestContext, point: Point<Pixels>) -> bool {
    cx.simulate_mouse_move(point, None, Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    cx.debug_bounds(TOOLTIP_SELECTOR).is_some()
}

/// The #267 shot for one trigger: the tooltip explains the trigger on hover, hides
/// while the trigger's own menu is open, and comes back when the menu is gone.
fn tooltip_tracks_the_menu(
    cx: &mut VisualTestContext,
    control_selector: &'static str,
    entry_selector: &'static str,
) {
    let control = control_bounds(control_selector, cx);
    let trigger = trigger_point(control);
    assert!(
        tooltip_after_hovering(cx, trigger),
        "the {control_selector} trigger explains itself on hover"
    );

    cx.simulate_click(trigger, Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(entry_selector).is_some(),
        "clicking the {control_selector} trigger opens its dropdown"
    );

    // The pointer rests on the trigger while the menu is open — the #267 shot.
    assert!(
        !tooltip_after_hovering(cx, trigger),
        "the {control_selector} tooltip hides while its own menu is open"
    );

    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(entry_selector).is_none(),
        "escape dismissed the {control_selector} menu"
    );
    assert!(
        tooltip_after_hovering(cx, trigger),
        "the tooltip comes back once the menu is gone — it is hidden, not dropped"
    );
}

#[gpui::test]
fn test_the_auth_type_tooltip_stays_out_of_its_own_menu(cx: &mut TestAppContext) {
    let cx = open_auth_tab(cx);
    tooltip_tracks_the_menu(cx, TYPE_CONTROL_SELECTOR, TYPE_MENU_ENTRY_SELECTOR);
}

#[gpui::test]
fn test_the_api_key_location_tooltip_stays_out_of_its_own_menu(cx: &mut TestAppContext) {
    let cx = open_auth_tab(cx);
    tooltip_tracks_the_menu(cx, LOCATION_CONTROL_SELECTOR, LOCATION_MENU_ENTRY_SELECTOR);
}
