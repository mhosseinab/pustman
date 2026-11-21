//! A text field's text is body UI text: the 14 px `TextSize::Default` every label
//! around it uses, not the larger `ui_font_size` (#211 — the URL bar and the search
//! fields read visibly bigger than their neighbours).

use gpui::{TestAppContext, px};
use ui::test_support::{draw_window, init_test};

use super::TextInput;
use crate::init;

#[gpui::test]
fn test_a_text_input_shapes_its_text_at_the_body_size(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        init(cx);
    });
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = TextInput::new(window, cx);
        input.set_text("https://api.example.com", cx);
        input
    });
    draw_window(cx);

    let font_size = cx.read(|cx| {
        input.read_with(cx, |input, _| {
            input
                .last_layout
                .borrow()
                .as_ref()
                .map(|line| line.font_size)
        })
    });
    assert_eq!(font_size, Some(px(14.)), "inputs use the 14 px body size");
}
