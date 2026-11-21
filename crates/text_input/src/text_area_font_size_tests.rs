//! A text area's text is body UI text, like a single-line input's (#211).

use gpui::{TestAppContext, px};
use ui::test_support::{draw_window, init_test};

use super::TextArea;
use crate::init;

#[gpui::test]
fn test_a_text_area_shapes_its_text_at_the_body_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (area, cx) = cx.add_window_view(|window, cx| {
        let mut area = TextArea::new(window, cx);
        area.set_text("{\"id\": 1}", cx);
        area
    });
    draw_window(cx);

    let font_size = cx.read(|cx| {
        area.read_with(cx, |area, _| {
            area.last_layout.borrow().as_ref().and_then(|layout| {
                layout
                    .lines
                    .first()
                    .map(|line| line.unwrapped_layout.font_size)
            })
        })
    });
    assert_eq!(
        font_size,
        Some(px(14.)),
        "text areas use the 14 px body size"
    );
}
