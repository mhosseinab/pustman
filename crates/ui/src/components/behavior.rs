//! The behavior every interactive component implements the same way.
//!
//! A button, a tab and a toggle each answer "am I turned off", "how wide am I" — with the
//! same method names on each, so a call site reads the same whoever it is calling
//! (`references/components.md`, authoring rule 3). Clicking is where a tab differs: its
//! activation belongs to the [`crate::TabBar`] — one path for the mouse and the keyboard —
//! so the bar, never the tab, attaches the handler.

use gpui::{AbsoluteLength, App, ClickEvent, CursorStyle, SharedString, Window};

use crate::components::toggle::ToggleState;

/// A control that can be clicked. The handler runs for a mouse click *and* for the
/// keyboard: Enter or Space pressed and released while the control is focused fires the
/// same handler once per press (GPUI's synthesized keyboard click), so the mouse and the
/// keyboard can never disagree — and holding the key down does not repeat it. Nested
/// controls stop propagation before running the handler, so a control inside a clickable
/// row never double-fires with its ancestor.
pub trait Clickable {
    /// Run `handler` when the control is activated.
    fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self;

    /// The cursor shown over the control.
    fn cursor_style(self, style: CursorStyle) -> Self;
}

/// A control that can be turned off. A disabled control keeps its place in the tab order
/// where that is meaningful, but fires no handler and draws in [`crate::Color::Disabled`].
pub trait Disableable {
    /// Turn the control off (or back on).
    fn disabled(self, disabled: bool) -> Self;
}

/// A control with on/off/indeterminate state.
pub trait Toggleable {
    /// Set the state: `true` is [`ToggleState::Selected`], `false` is
    /// [`ToggleState::Unselected`].
    fn toggle_state(self, state: impl Into<ToggleState>) -> Self;
}

/// A control whose width can be pinned.
pub trait FixedWidth {
    /// Pin the width.
    fn width(self, width: impl Into<AbsoluteLength>) -> Self;

    /// Take the width of whatever contains it.
    fn full_width(self) -> Self;
}

/// A control that is revealed when its row is hovered.
pub trait VisibleOnHover {
    /// Out of the frame — no hit target, no tab stop — until `group` (the row's
    /// `.group(...)`) is hovered: for a control the row shows in this one's place at rest.
    fn visible_on_hover(self, group: SharedString) -> Self;

    /// Faded out until `group` (the row's `.group(...)`) is hovered, or the control takes
    /// keyboard focus: the slot keeps its hit target and tab order while the glyph is
    /// hidden, so a click that lands before the reveal still works and the focus ring is
    /// visible where it lands.
    fn faded_until_hovered(self, group: SharedString) -> Self;
}
