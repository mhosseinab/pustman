//! The shared `floating_menu` (#177): one floating shape for every picker menu —
//! anchored just under its trigger so the editor below cannot clip it, snapped to the
//! window edge so it never leaves the window. Every dropdown in the app floats this
//! one way (`url_bar`'s method picker and the request editor's mode, language,
//! auth-type, key-location and version pickers).

use gpui::{
    AppContext, Bounds, Context, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, Styled, TestAppContext, VisualTestContext, Window, div, px,
    size,
};

use crate::components::context_menu::{ContextMenu, floating_menu};
use crate::h_flex;
use crate::test_support::{draw_window, init_test};
use crate::units::px_to_rems;

/// The width of the probe's trigger and the menu's widest entry, in pixels at the
/// default scale.
const TRIGGER_WIDTH_PX: f32 = 160.;

/// The height of the probe's trigger: what the menu floats below.
const TRIGGER_HEIGHT_PX: f32 = 28.;

/// A window with one picker trigger and the menu floating under it — the shape every
/// `floating_menu` caller renders.
struct FloatingMenuProbe {
    menu: Entity<ContextMenu>,
    /// Whether the trigger ends the row instead of starting it (the Beautify-end
    /// layout a menu must still snap inside the window for).
    pin_right: bool,
}

impl Render for FloatingMenuProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The caller's shape: a relative wrapper that hugs its trigger, so the menu's
        // `top(trigger_height)` lands exactly under the trigger.
        let picker = h_flex()
            .relative()
            .child(
                div()
                    .id(ElementId::Name(SharedString::new_static("menu-trigger")))
                    .debug_selector(|| String::from("menu-trigger"))
                    .w(px_to_rems(TRIGGER_WIDTH_PX))
                    .h(px_to_rems(TRIGGER_HEIGHT_PX)),
            )
            .children(floating_menu(
                &Some(self.menu.clone()),
                px_to_rems(TRIGGER_HEIGHT_PX),
                cx,
            ));
        let row = h_flex().size_full();
        let row = if self.pin_right {
            row.justify_end()
        } else {
            row
        };
        row.child(picker)
    }
}

/// The probe's trigger and one menu entry, in their own window of `size`.
fn open_probe(
    cx: &mut TestAppContext,
    pin_right: bool,
    window_size: Size<Pixels>,
) -> &mut VisualTestContext {
    cx.update(init_test);
    let (_probe, cx) = cx.add_window_view(|_window, cx| FloatingMenuProbe {
        menu: cx.new(|cx| ContextMenu::new(cx).entry("Open", |_, _, _| {})),
        pin_right,
    });
    cx.simulate_resize(window_size);
    draw_window(cx);
    cx
}

/// The painted bounds of `selector`.
fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

/// The menu entry's painted bounds: proof the menu painted at all.
fn entry_bounds(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    bounds("Open", cx)
}

#[gpui::test]
fn test_the_floating_menu_paints_just_under_its_trigger(cx: &mut TestAppContext) {
    let cx = open_probe(cx, false, size(px(800.), px(600.)));

    let trigger = bounds("menu-trigger", cx);
    let entry = entry_bounds(cx);

    assert!(
        entry.origin.y >= trigger.origin.y + trigger.size.height,
        "the menu floats under its trigger: trigger {trigger:?}, entry {entry:?}"
    );
    assert!(
        entry.origin.x >= trigger.origin.x,
        "the menu anchors at its trigger's left edge: trigger {trigger:?}, entry {entry:?}"
    );
}

#[gpui::test]
fn test_the_floating_menu_stays_inside_the_window(cx: &mut TestAppContext) {
    let window = size(px(800.), px(600.));
    let cx = open_probe(cx, true, window);

    let trigger = bounds("menu-trigger", cx);
    let entry = entry_bounds(cx);

    assert!(
        trigger.origin.x + trigger.size.width == px(800.),
        "the trigger ends the row, where a menu would overflow the window: {trigger:?}"
    );
    assert!(
        entry.origin.x >= px(0.) && entry.origin.x + entry.size.width <= px(800.),
        "the menu snaps back inside the window: {entry:?}"
    );
    assert!(
        entry.origin.y >= px(0.) && entry.origin.y + entry.size.height <= px(600.),
        "the menu paints inside the window: {entry:?}"
    );
}
