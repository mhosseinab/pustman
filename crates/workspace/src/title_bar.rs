//! The window frame Pustman draws when the platform leaves decorations to the app — GNOME
//! on Wayland has no server-side decorations, so without this the window cannot be moved,
//! resized or closed. [`TitleBar`] is the strip along the top (drag to move, double-click
//! to maximize, right-click for the window menu, minimize/maximize/close buttons) and
//! [`client_frame`] is the invisible band around the content that resizes the window.
//! The shape follows GPUI's Apache-2.0 `window_shadow` example; the code is our own.

use gpui::{
    AnyElement, App, Bounds, Context, CursorStyle, HitboxBehavior, IntoElement, MouseButton,
    ParentElement, Pixels, Point, Render, ResizeEdge, Size, Tiling, Window, canvas, point, px,
};
use ui::prelude::*;

use crate::CloseWindow;

/// The title bar's stable element id and debug selector.
pub(crate) const TITLE_BAR_SELECTOR: &str = "title-bar";
/// The window control buttons: element ids and debug selectors both.
pub(crate) const MINIMIZE_SELECTOR: &str = "title-bar-minimize";
pub(crate) const MAXIMIZE_SELECTOR: &str = "title-bar-maximize";
pub(crate) const CLOSE_SELECTOR: &str = "title-bar-close";

/// The redesign pack's `layout.titleBar` height.
const TITLE_BAR_HEIGHT_PX: f32 = 32.;

/// How wide the resize band around an untiled window is. It is window geometry the
/// compositor is told about (`set_client_inset`), not layout spacing, so it is fixed
/// pixels rather than a density-scaled `DynamicSpacing` step.
pub(crate) const RESIZE_INSET_PX: f32 = 8.;

/// The window edge or corner `position` resizes, or `None` over the content. A tiled edge
/// has no band (the compositor pins it), so it never resizes.
pub(crate) fn resize_edge(
    position: Point<Pixels>,
    inset: Pixels,
    size: Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let top = !tiling.top && position.y < inset;
    let bottom = !tiling.bottom && position.y > size.height - inset;
    let left = !tiling.left && position.x < inset;
    let right = !tiling.right && position.x > size.width - inset;
    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (true, _, _, true) => Some(ResizeEdge::TopRight),
        (true, _, _, _) => Some(ResizeEdge::Top),
        (_, true, true, _) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (_, true, _, _) => Some(ResizeEdge::Bottom),
        (_, _, true, _) => Some(ResizeEdge::Left),
        (_, _, _, true) => Some(ResizeEdge::Right),
        _ => None,
    }
}

fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// Wrap the workspace `content` in the resize band a client-decorated window needs.
pub(crate) fn client_frame(
    tiling: Tiling,
    content: impl IntoElement,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let inset = px(RESIZE_INSET_PX);
    // A request to the platform about the surface's geometry, not a state change: it does
    // not notify, so it is safe in render (GPUI's own example sets it there).
    window.set_client_inset(inset);
    let colors = cx.theme().colors();

    div()
        .id("window-frame")
        .size_full()
        .when(!tiling.top, |frame| frame.pt(inset))
        .when(!tiling.bottom, |frame| frame.pb(inset))
        .when(!tiling.left, |frame| frame.pl(inset))
        .when(!tiling.right, |frame| frame.pr(inset))
        .child(
            canvas(
                |_bounds, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    window.insert_hitbox(
                        Bounds::new(point(px(0.), px(0.)), size),
                        HitboxBehavior::Normal,
                    )
                },
                move |_bounds, hitbox, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    if let Some(edge) = resize_edge(window.mouse_position(), inset, size, tiling) {
                        window.set_cursor_style(resize_cursor(edge), &hitbox);
                    }
                },
            )
            .size_full()
            .absolute(),
        )
        // Repaint so the cursor follows the band; the content stops these moves below.
        .on_mouse_move(|_event, window, _cx| window.refresh())
        .on_mouse_down(MouseButton::Left, move |event, window, _cx| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(event.position, inset, size, tiling) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .cursor(CursorStyle::Arrow)
                .border_color(colors.border)
                .when(!tiling.top, |content| content.border_t_1())
                .when(!tiling.bottom, |content| content.border_b_1())
                .when(!tiling.left, |content| content.border_l_1())
                .when(!tiling.right, |content| content.border_r_1())
                .when(!tiling.is_tiled(), |content| content.shadow_md())
                .on_mouse_move(|_event, _window, cx| cx.stop_propagation())
                .child(content),
        )
        .into_any_element()
}

/// The strip along the top of a client-decorated window.
pub struct TitleBar {
    /// Set by a press on the drag area and spent by the first move after it: starting the
    /// compositor's move on the press itself would swallow the second click of a
    /// double-click, so maximize-by-double-click could never fire. Any left-button release
    /// disarms it — `on_mouse_up` over the drag area, `on_mouse_up_out` anywhere else — so a
    /// press released elsewhere can never arm a move on a later hover.
    should_move: bool,
}

impl TitleBar {
    /// A title bar with no drag in progress.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self { should_move: false }
    }
}

fn window_control(
    selector: &'static str,
    icon: IconName,
    label: &'static str,
    action: fn(&mut Window, &mut App),
) -> impl IntoElement {
    div().debug_selector(|| selector.to_owned()).child(
        IconButton::new(selector, icon, Tooltip::text(label))
            .on_click(move |_event, window, cx| action(window, cx)),
    )
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let controls = window.window_controls();
        let maximize_label = if window.is_maximized() {
            "Restore"
        } else {
            "Maximize"
        };

        h_flex()
            .id(TITLE_BAR_SELECTOR)
            .debug_selector(|| TITLE_BAR_SELECTOR.to_owned())
            .w_full()
            .h(px_to_rems(TITLE_BAR_HEIGHT_PX))
            .flex_none()
            .bg(colors.title_bar_background)
            .border_b_1()
            .border_color(colors.border_variant)
            .pr(DynamicSpacing::Base02.rems(cx))
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(
                h_flex()
                    .id("title-bar-drag")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl(DynamicSpacing::Base08.rems(cx))
                    .child(
                        Label::new("Pustman")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, _cx| this.should_move = true),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, _cx| this.should_move = false),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _event, _window, _cx| this.should_move = false),
                    )
                    .on_mouse_move(cx.listener(|this, _event, window, _cx| {
                        if this.should_move {
                            this.should_move = false;
                            window.start_window_move();
                        }
                    }))
                    .on_click(|event, window, _cx| {
                        if event.click_count() == 2 {
                            window.zoom_window();
                        }
                    })
                    .when(controls.window_menu, |drag| {
                        drag.on_mouse_down(MouseButton::Right, |event, window, _cx| {
                            window.show_window_menu(event.position);
                        })
                    }),
            )
            .when(controls.minimize, |bar| {
                bar.child(window_control(
                    MINIMIZE_SELECTOR,
                    IconName::Minus,
                    "Minimize",
                    |window, _cx| window.minimize_window(),
                ))
            })
            .when(controls.maximize, |bar| {
                bar.child(window_control(
                    MAXIMIZE_SELECTOR,
                    IconName::Square,
                    maximize_label,
                    |window, _cx| window.zoom_window(),
                ))
            })
            .child(window_control(
                CLOSE_SELECTOR,
                IconName::X,
                "Close",
                // The app's own close button must not tear the window down directly:
                // dispatching the close action makes the shell run the same
                // save-then-close flush a platform close takes (issue #85).
                |window, cx| window.dispatch_action(Box::new(CloseWindow), cx),
            ))
    }
}

#[cfg(test)]
#[path = "title_bar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "title_bar_move_tests.rs"]
mod move_tests;

#[cfg(test)]
#[path = "title_bar_close_tests.rs"]
mod close_tests;
