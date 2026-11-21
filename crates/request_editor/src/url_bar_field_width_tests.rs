//! The URL field's text box fills the bordered field it sits in. Centring the box
//! vertically (#218) must not shrink it to its content: a collapsed box clips the
//! placeholder and leaves most of the field dead to clicks.

use api_model::Method;
use gpui::{Bounds, Focusable, Pixels, TestAppContext, VisualTestContext, px};
use text_input::input::TEXT_INPUT_SELECTOR;
use ui::test_support::{draw_window, init_test};

use super::{URL_FIELD_SELECTOR, UrlBar};

/// The field's border and inner padding together stay well under this.
const MAX_INSET: Pixels = px(24.);

fn url_bar_window(cx: &mut TestAppContext) -> (gpui::Entity<UrlBar>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (bar, cx) =
        cx.add_window_view(|window, cx| UrlBar::new(Method::Get, String::new(), window, cx));
    draw_window(cx);
    (bar, cx)
}

fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

#[gpui::test]
fn test_url_text_box_spans_the_whole_url_field(cx: &mut TestAppContext) {
    let (_bar, cx) = url_bar_window(cx);
    let field = bounds(cx, URL_FIELD_SELECTOR);
    let text_box = bounds(cx, TEXT_INPUT_SELECTOR);

    assert!(
        text_box.size.width >= field.size.width - MAX_INSET,
        "the text box ({:?} wide) fills the URL field ({:?} wide)",
        text_box.size.width,
        field.size.width
    );
}

#[gpui::test]
fn test_clicking_the_middle_of_the_url_field_focuses_it(cx: &mut TestAppContext) {
    let (bar, cx) = url_bar_window(cx);
    let field = bounds(cx, URL_FIELD_SELECTOR);

    cx.simulate_click(field.center(), gpui::Modifiers::none());

    let focused = cx.update(|window, cx| {
        bar.read(cx)
            .url_input()
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    });
    assert!(
        focused,
        "a click anywhere in the URL field puts typing there"
    );
}
