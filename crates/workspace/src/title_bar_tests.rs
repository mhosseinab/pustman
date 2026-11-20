//! The title bar Pustman draws when the platform leaves decorations to the app (GNOME on
//! Wayland): without it the window cannot be moved, resized or closed. Minimize, maximize
//! and move are compositor requests the test platform cannot perform, so these tests
//! check the controls are there and close the window through the one control the test
//! platform does carry out.

use gpui::{
    Decorations, Modifiers, ResizeEdge, Size, TestAppContext, Tiling, VisualTestContext, point, px,
};
use ui::test_support::{draw_window, init_test};

use crate::title_bar::{
    CLOSE_SELECTOR, MAXIMIZE_SELECTOR, MINIMIZE_SELECTOR, RESIZE_INSET_PX, TITLE_BAR_SELECTOR,
    resize_edge,
};
use crate::{Workspace, init};

const WINDOW: Size<gpui::Pixels> = Size {
    width: px(800.),
    height: px(600.),
};

fn workspace_window(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(init);
    let (_workspace, cx) = cx.add_window_view(Workspace::new);
    cx
}

fn decorate_by_client(cx: &mut VisualTestContext, tiling: Tiling) {
    let window = cx.windows()[0];
    cx.simulate_window_decorations_change(window, Decorations::Client { tiling });
    draw_window(cx);
}

#[test]
fn test_pointer_in_the_inset_band_picks_the_edge_or_corner_under_it() {
    let inset = px(RESIZE_INSET_PX);
    let free = Tiling::default();
    assert_eq!(
        resize_edge(point(px(2.), px(2.)), inset, WINDOW, free),
        Some(ResizeEdge::TopLeft)
    );
    assert_eq!(
        resize_edge(point(px(798.), px(2.)), inset, WINDOW, free),
        Some(ResizeEdge::TopRight)
    );
    assert_eq!(
        resize_edge(point(px(400.), px(2.)), inset, WINDOW, free),
        Some(ResizeEdge::Top)
    );
    assert_eq!(
        resize_edge(point(px(2.), px(598.)), inset, WINDOW, free),
        Some(ResizeEdge::BottomLeft)
    );
    assert_eq!(
        resize_edge(point(px(798.), px(598.)), inset, WINDOW, free),
        Some(ResizeEdge::BottomRight)
    );
    assert_eq!(
        resize_edge(point(px(400.), px(598.)), inset, WINDOW, free),
        Some(ResizeEdge::Bottom)
    );
    assert_eq!(
        resize_edge(point(px(2.), px(300.)), inset, WINDOW, free),
        Some(ResizeEdge::Left)
    );
    assert_eq!(
        resize_edge(point(px(798.), px(300.)), inset, WINDOW, free),
        Some(ResizeEdge::Right)
    );
    assert_eq!(
        resize_edge(point(px(400.), px(300.)), inset, WINDOW, free),
        None,
        "the window's content is never a resize handle"
    );
}

#[test]
fn test_a_tiled_edge_is_never_a_resize_handle() {
    let inset = px(RESIZE_INSET_PX);
    let top_tiled = Tiling {
        top: true,
        ..Tiling::default()
    };
    assert_eq!(
        resize_edge(point(px(400.), px(2.)), inset, WINDOW, top_tiled),
        None,
        "a top-tiled window has no top band: a click there is a title bar click"
    );
    assert_eq!(
        resize_edge(point(px(2.), px(2.)), inset, WINDOW, top_tiled),
        Some(ResizeEdge::Left)
    );
    assert_eq!(
        resize_edge(point(px(400.), px(300.)), inset, WINDOW, Tiling::tiled()),
        None
    );
}

#[gpui::test]
fn test_a_server_decorated_window_draws_no_title_bar(cx: &mut TestAppContext) {
    let cx = workspace_window(cx);
    draw_window(cx);
    assert!(
        cx.debug_bounds(TITLE_BAR_SELECTOR).is_none(),
        "the platform draws the frame; a second title bar would double it"
    );
}

#[gpui::test]
fn test_a_client_decorated_window_draws_a_title_bar_with_window_controls(cx: &mut TestAppContext) {
    let cx = workspace_window(cx);
    decorate_by_client(cx, Tiling::default());

    let title_bar = cx
        .debug_bounds(TITLE_BAR_SELECTOR)
        .expect("a client-decorated window draws its own title bar");
    // The resize band, then the 1px window border, then the content.
    let content_origin = px(RESIZE_INSET_PX + 1.);
    assert_eq!(
        title_bar.origin,
        point(content_origin, content_origin),
        "the title bar is the top of the content, inside the resize band and the border"
    );
    for selector in [MINIMIZE_SELECTOR, MAXIMIZE_SELECTOR, CLOSE_SELECTOR] {
        let control = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the title bar shows {selector}"));
        assert!(
            title_bar.contains(&control.center()),
            "{selector} sits in the title bar"
        );
    }
}

#[gpui::test]
fn test_a_tiled_window_drops_the_resize_band(cx: &mut TestAppContext) {
    let cx = workspace_window(cx);
    decorate_by_client(cx, Tiling::tiled());

    let title_bar = cx
        .debug_bounds(TITLE_BAR_SELECTOR)
        .expect("a tiled window still needs its controls");
    assert_eq!(title_bar.origin, point(px(0.), px(0.)));
}

#[gpui::test]
fn test_the_close_button_closes_the_window(cx: &mut TestAppContext) {
    let cx = workspace_window(cx);
    decorate_by_client(cx, Tiling::default());

    let close = cx
        .debug_bounds(CLOSE_SELECTOR)
        .expect("the title bar shows a close button");
    cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();

    assert!(cx.windows().is_empty(), "clicking close removes the window");
}
