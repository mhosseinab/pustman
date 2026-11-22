//! The move half of the title bar's drag contract: a press on the drag area arms a window
//! move that the first move over the bar spends, and a release anywhere disarms it. The
//! test platform panics in `start_window_move`, so "no move was started" is proven by
//! getting through the sequence without that panic; actually moving the window stays
//! manual smoke (umbrella #241, #107).

use gpui::{
    Decorations, Modifiers, MouseButton, TestAppContext, Tiling, VisualTestContext, point, px,
};
use ui::test_support::{draw_window, init_test};

use crate::title_bar::TITLE_BAR_SELECTOR;
use crate::{Workspace, init};

fn workspace_window(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(init);
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    cx
}

#[gpui::test]
fn test_a_press_released_outside_the_drag_area_does_not_arm_a_later_window_move(
    cx: &mut TestAppContext,
) {
    let cx = workspace_window(cx);
    let window = cx.windows()[0];
    cx.simulate_window_decorations_change(
        window,
        Decorations::Client {
            tiling: Tiling::default(),
        },
    );
    draw_window(cx);

    let title_bar = cx
        .debug_bounds(TITLE_BAR_SELECTOR)
        .expect("a client-decorated window draws its own title bar");
    // Left of the window controls and clear of the leading padding: the drag area.
    let drag_area = point(title_bar.origin.x + px(200.), title_bar.center().y);
    let outside = point(px(400.), px(400.));

    cx.simulate_mouse_down(drag_area, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());

    // A plain hover over the drag area after that release must not start a window move:
    // TestWindow panics "not implemented" in `start_window_move`, so a hover that arms the
    // stale move fails this test there instead of dragging the window along.
    cx.simulate_mouse_move(drag_area, None::<MouseButton>, Modifiers::none());
    cx.run_until_parked();

    assert!(
        cx.debug_bounds(TITLE_BAR_SELECTOR).is_some(),
        "the title bar still draws after the aborted drag"
    );
}
