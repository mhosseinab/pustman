//! [`TextArea`] in real layout: the widget must fill the box its owner gives it. A root
//! with no size resolved 100 % of an unsized box to 0 px, so a raw body editor was 0 px
//! tall — nothing could be typed into or seen (S33n). The tests paint the widget in a
//! sized host and read its painted bounds back, and click it the way a user would.

use gpui::{
    AppContext, Context, Entity, IntoElement, Modifiers, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, point, px,
};
use ui::spacing::DynamicSpacing;
use ui::test_support::{draw_window, init_test};

use super::TextArea;
use crate::init;

/// The box the host gives the text area, in pixels at the default scale: big enough
/// that a 0 px widget cannot accidentally pass.
const HOST_SIZE_PX: f32 = 240.;

/// One text area in a box of [`HOST_SIZE_PX`]: the shape every text area host has — the
/// raw body editor wraps it in a sized, bordered div the same way.
struct SizedHost {
    area: Entity<TextArea>,
}

impl Render for SizedHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .w(px(HOST_SIZE_PX))
                .h(px(HOST_SIZE_PX))
                .child(self.area.clone()),
        )
    }
}

/// A window with one text area in its sized host, drawn — the widget's painted bounds
/// are then observable through [`TextArea::painted_bounds`].
fn open_host(cx: &mut TestAppContext) -> (Entity<TextArea>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (host, cx) = cx.add_window_view(|window, cx| SizedHost {
        area: cx.new(|cx| TextArea::new(window, cx)),
    });
    draw_window(cx);
    let area = cx.read(|cx| host.read(cx).area.clone());
    (area, cx)
}

#[gpui::test]
fn test_text_area_fills_the_box_its_host_gives_it(cx: &mut TestAppContext) {
    let (area, cx) = open_host(cx);

    let bounds = cx
        .read(|cx| area.read(cx).painted_bounds())
        .expect("the text area painted");
    let side_padding = cx.read(|cx| DynamicSpacing::Base04.px(cx));
    assert_eq!(
        bounds.size.height,
        px(HOST_SIZE_PX),
        "the text area fills its host's height — a 0 px editor cannot be typed into or seen"
    );
    assert_eq!(
        bounds.size.width,
        px(HOST_SIZE_PX) - side_padding * 2.,
        "the text area fills its host's width, inside its own side padding"
    );
}

#[gpui::test]
fn test_clicking_the_text_area_focuses_it_for_typing(cx: &mut TestAppContext) {
    let (area, cx) = open_host(cx);

    // The click a user makes to start editing: the middle of the editor's box.
    let center = point(px(HOST_SIZE_PX / 2.), px(HOST_SIZE_PX / 2.));
    cx.simulate_click(center, Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    cx.simulate_input("typed");
    cx.run_until_parked();

    let text = cx.read(|cx| area.read(cx).text());
    assert_eq!(
        text, "typed",
        "a click in the editor reaches the widget, so typing lands in it"
    );
}
