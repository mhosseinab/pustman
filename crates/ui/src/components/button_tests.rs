//! A button answers Enter and Space through the same handler a mouse click runs — one
//! activation per press-and-release while it is focused — and a disabled button answers
//! nothing at all (`components/behavior.rs`).

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Context, FocusHandle, IntoElement, ParentElement, Render, TestAppContext, VisualTestContext,
    Window,
};

use crate::components::behavior::Disableable;
use crate::components::button::{Button, Clickable};
use crate::h_flex;
use crate::test_support::{draw_window, init_test, press_key};

/// One button in a window of its own, counting the activations its handler sees.
struct ButtonHarness {
    focus_handle: FocusHandle,
    enabled: bool,
    activations: Rc<Cell<usize>>,
}

impl Render for ButtonHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let activations = self.activations.clone();
        h_flex().child(
            Button::new("run", "Run")
                .focus_handle(self.focus_handle.clone())
                .disabled(!self.enabled)
                .on_click(move |_, _, _| activations.set(activations.get() + 1)),
        )
    }
}

/// Open a window with one button — `enabled` or disabled — drawn and focused.
fn focus_button(
    cx: &mut TestAppContext,
    enabled: bool,
) -> (Rc<Cell<usize>>, &mut VisualTestContext) {
    let activations = Rc::new(Cell::new(0));
    let (button, cx) = cx.add_window_view(|_, cx| ButtonHarness {
        focus_handle: cx.focus_handle(),
        enabled,
        activations: activations.clone(),
    });
    draw_window(cx);
    let focus_handle = cx.read(|cx| button.read_with(cx, |button, _| button.focus_handle.clone()));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (activations, cx)
}

#[gpui::test]
fn a_focused_button_activates_once_per_enter_and_space_press(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (activations, cx) = focus_button(cx, true);

    press_key(cx, "enter");
    assert_eq!(activations.get(), 1, "Enter activates exactly once");

    press_key(cx, "space");
    assert_eq!(activations.get(), 2, "Space activates exactly once");
}

#[gpui::test]
fn a_focused_disabled_button_ignores_enter_and_space(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (activations, cx) = focus_button(cx, false);

    press_key(cx, "enter");
    press_key(cx, "space");
    assert_eq!(activations.get(), 0, "a disabled button never activates");
}
