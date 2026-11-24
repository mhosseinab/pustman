//! `command_palette` — the reusable modal [`Picker`] and the [`fuzzy`] scorer it ranks with.
//!
//! `Decision:` the picker lives here and not in `ui`. A picker owns an
//! `Entity<TextInput>`, and `ui` must not depend on `text_input` — dependencies only
//! point down (`pustman-feature-crate` skill). S34a's command palette, S41b and S46
//! depend on this crate to reuse the picker instead of growing their own.
//!
//! `Decision:` matching is synchronous: a keystroke re-ranks the delegate's candidates
//! before the next frame. The candidate lists behind v1 pickers (commands, environments)
//! are small enough for that, and a background matcher with stale-result handling is a
//! later improvement, not this step.
//!
//! Two doors in:
//! - [`picker`] — the [`Picker`] entity and the [`PickerDelegate`] a host implements.
//! - [`fuzzy`] — the scorer that ranks the delegate's candidates.
//!
//! plus [`init`] — called once at startup after `ui` and `text_input`; it binds the keys
//! the picker answers to under its own [`PICKER_CONTEXT`].

pub mod fuzzy;
pub mod palette;
pub mod picker;
pub mod recent;

pub use picker::{PICKER_CONTEXT, Picker, PickerDelegate};

use gpui::{App, KeyBinding};

use picker::{Cancel, Confirm, SelectNext, SelectPrevious};

/// Set the picker up: bind the keys every [`Picker`] answers to.
///
/// `Decision:` like `text_input`'s editing keys, these bindings belong to the widget and
/// not to `assets/keymaps/*` — they are picker navigation semantics, not PRD actions a
/// user remaps. The `"Picker"` context sits above the query field's `"TextInput"`
/// context: the arrows and Escape reach the picker, while typing and Enter belong to the
/// focused field (Enter reaches [`PickerDelegate::confirm`] through the field's
/// `Submitted` event).
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some(PICKER_CONTEXT)),
        KeyBinding::new("up", SelectPrevious, Some(PICKER_CONTEXT)),
        KeyBinding::new("enter", Confirm, Some(PICKER_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(PICKER_CONTEXT)),
    ]);
    palette::init(cx);
    recent::init(cx);
}

#[cfg(test)]
#[path = "recent_open_tests.rs"]
mod recent_open_tests;
