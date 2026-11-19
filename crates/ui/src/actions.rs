//! The keyboard actions Pustman's own components answer to.
//!
//! These are *component* actions, not PRD actions: they live in the key contexts the
//! components own (`"TabBar"`, `"Menu"`) and are bound by [`crate::init`].
//! The PRD's command actions land in `pustman_actions` (S21) and are dispatched to views.
//! [`Activate`] confirms the selected item of a composite control like [`crate::TabBar`].
//! Buttons and toggles are not driven by actions: GPUI synthesizes a click on the focused
//! control when Enter or Space is pressed and released — one per press, never on key
//! auto-repeat (`references/interaction-focus-a11y.md`).

use gpui::actions;

actions!(ui, [Activate, SelectNext, SelectPrevious, Cancel]);
