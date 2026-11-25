//! The labelled form field, as it paints: the caption and help text of a quiet field,
//! and the inline error of a broken one — the error taking the hint's place under the
//! box and never sitting on top of the field it is about.

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, TestAppContext, VisualTestContext, Window,
    div, px,
};

use super::FormField;
use crate::h_flex;
use crate::test_support::{draw_window, init_test};

/// The help text under the healthy field.
const HINT: &str = "The folder the workspace is created in";

/// The problem the second field reports inline.
const ERROR: &str = "Enter a name for the workspace";

/// The two fields, side by side, each box tall enough to measure against its message.
struct FieldHarness;

impl Render for FieldHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap(px(16.))
            .child(
                FormField::new("location", "Location")
                    .hint(HINT)
                    .content(div().w_full().h(px(24.))),
            )
            .child(
                FormField::new("name", "Name")
                    .hint("What the workspace is called")
                    .error(ERROR)
                    .content(div().w_full().h(px(24.))),
            )
    }
}

fn open_harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    let (_harness, cx) = cx.add_window_view(|_window, _cx| FieldHarness);
    draw_window(cx);
    cx
}

/// `debug_bounds` keys live for the whole test, so a selector the frame painted under
/// is leaked the way the panel tests leak theirs.
fn selector(name: &str) -> &'static str {
    String::leak(name.to_owned())
}

#[gpui::test]
fn form_field_paints_caption_help_text_and_the_field(cx: &mut TestAppContext) {
    let cx = open_harness(cx);

    assert!(
        cx.debug_bounds(selector(&FormField::label_selector("location")))
            .is_some(),
        "the caption paints over the field"
    );
    assert!(
        cx.debug_bounds(selector(&FormField::hint_selector("location")))
            .is_some(),
        "the help text paints under the field"
    );
}

#[gpui::test]
fn form_field_inline_error_replaces_the_hint_under_the_field(cx: &mut TestAppContext) {
    let cx = open_harness(cx);

    let error = cx
        .debug_bounds(selector(&FormField::error_selector("name")))
        .expect("the inline error paints with its field");
    assert!(
        cx.debug_bounds(selector(&FormField::hint_selector("name")))
            .is_none(),
        "the error takes the help text's place — one message under the field"
    );
    let label = cx
        .debug_bounds(selector(&FormField::label_selector("name")))
        .expect("the caption paints");
    assert!(
        error.origin.y > label.origin.y,
        "the error sits below the caption, next to the field it is about: {error:?} vs {label:?}"
    );
}
