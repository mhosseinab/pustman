//! The toggle's label, both modes: drawn beside the control by default, and the
//! accessible name alone when the caller asks for it (S33n) — a table checkbox's column
//! header ("Enabled") is its visible meaning, and painted label text would run over the
//! cells beside it. The tests measure what paints: each toggle's real width in a row,
//! and the label element's painted bounds under its debug key.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, TestAppContext,
    VisualTestContext, Window, div, px,
};

use super::{CONTROL_PX, Toggle, ToggleState, toggle_label_selector};
use crate::h_flex;
use crate::test_support::{draw_window, init_test};

/// The label the first toggle keeps as its accessible name only.
const HIDDEN_LABEL: &str = "Include this row in the request";

/// The label the second toggle draws beside its control.
const VISIBLE_LABEL: &str = "Follow redirects";

const HIDDEN_WRAP_SELECTOR: &str = "toggle-label-hidden-wrap";
const VISIBLE_WRAP_SELECTOR: &str = "toggle-label-visible-wrap";

/// Both label modes side by side in one row, each toggle keeping its natural width.
struct ToggleLabelHarness;

impl Render for ToggleLabelHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .child(
                div()
                    .debug_selector(|| String::from(HIDDEN_WRAP_SELECTOR))
                    .child(
                        Toggle::new("hidden", HIDDEN_LABEL, ToggleState::Unselected).label_hidden(),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| String::from(VISIBLE_WRAP_SELECTOR))
                    .child(Toggle::new(
                        "visible",
                        VISIBLE_LABEL,
                        ToggleState::Unselected,
                    )),
            )
    }
}

fn open_harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    let (_harness, cx) = cx.add_window_view(|_window, _cx| ToggleLabelHarness);
    draw_window(cx);
    cx
}

#[gpui::test]
fn test_hidden_label_contributes_no_width(cx: &mut TestAppContext) {
    let cx = open_harness(cx);

    let hidden = cx
        .debug_bounds(HIDDEN_WRAP_SELECTOR)
        .expect("the hidden-label toggle is on screen");
    let visible = cx
        .debug_bounds(VISIBLE_WRAP_SELECTOR)
        .expect("the visible-label toggle is on screen");
    assert!(
        hidden.size.width <= px(CONTROL_PX + 4.),
        "an accessible-name-only label adds no width — only the control paints: {hidden:?}"
    );
    assert!(
        visible.size.width > hidden.size.width + px(50.),
        "the drawn label is what widens a visible-label toggle: {visible:?}"
    );
}

#[gpui::test]
fn test_hidden_label_paints_no_text_while_the_visible_one_does(cx: &mut TestAppContext) {
    let cx = open_harness(cx);

    assert!(
        cx.debug_bounds(toggle_label_selector(VISIBLE_LABEL).leak())
            .is_some(),
        "the visible label paints its text — the hook that measures painted labels works"
    );
    assert!(
        cx.debug_bounds(toggle_label_selector(HIDDEN_LABEL).leak())
            .is_none(),
        "the hidden label paints no text — it is the control's accessible name only"
    );
}
