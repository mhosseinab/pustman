//! `text_input` — the text fields every screen edits through.
//!
//! Three doors in:
//! - [`input`] — the [`EditState`] editing core (shared by both widgets) and the
//!   [`TextInput`] single-line field: typing, cursor motion, shift selection, IME
//!   through [`gpui::EntityInputHandler`], a placeholder, read-only mode, and the
//!   [`TextInputEvent`]s (`Changed`, `Submitted`) an owner subscribes to.
//! - [`text_area`] — the [`TextArea`] multi-line widget over the same editing core:
//!   line motion, soft wrap and an optional line-number gutter, with Enter inserting a
//!   line break instead of submitting.
//! - [`init`] — called once at startup, after `ui`: it binds the editing keys both
//!   widgets answer to, under their own key contexts ([`input::TEXT_INPUT_CONTEXT`] and
//!   [`text_area::TEXT_AREA_CONTEXT`]).
//!
//! The buffer under them ([`buffer`]) is a gap buffer, so typing at the cursor never
//! copies the whole text; see the module docs there for the decision. Undo and redo run
//! on the transaction model in [`editing`]; copy, cut and paste go through the same
//! edit path and the platform clipboard. [`highlights`] holds the anchored highlight
//! spans both widgets paint and move through every edit.

pub mod buffer;
pub mod completion;
pub mod editing;
pub mod highlights;
pub mod input;
pub mod text_area;

pub use highlights::Highlight;
pub use input::{EditState, TextInput, TextInputEvent};
pub use text_area::TextArea;

use gpui::{App, Div, KeyBinding};
use ui::prelude::*;

use input::{
    Backspace, Copy, Cut, Delete, Down, End, Enter, Home, Left, Paste, Redo, Right, SelectAll,
    SelectLeft, SelectRight, SelectToEnd, SelectToHome, SelectWordLeft, SelectWordRight,
    TEXT_INPUT_CONTEXT, Undo, Up, WordLeft, WordRight,
};
use text_area::TEXT_AREA_CONTEXT;

/// Test hook: `VisualTestContext::debug_bounds` finds a focused field's ring under this key.
pub const FOCUS_RING_SELECTOR: &str = "text-field-focus-ring";

/// The ring a focused field draws over itself. It is an overlay, so gaining focus never
/// moves the text, and it shows however focus arrived: a click focuses a field as much
/// as Tab does, and either way the user needs to see where typing goes (#212).
fn focus_ring(cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded_sm()
        .border_1()
        .border_color(cx.theme().colors().border_focused)
        .debug_selector(|| FOCUS_RING_SELECTOR.to_string())
}

/// Set the widgets up: bind the editing keys every [`TextInput`] and [`TextArea`]
/// answers to.
///
/// `Decision:` these bindings belong to the widgets, not to `assets/keymaps/*` — they
/// are the editing semantics of a text field (Zed binds its `Editor` the same way), not
/// PRD actions a user remaps, so `assets/keymaps/*` (S21) is not edited here. Both key
/// contexts bind the same keys to the same actions; the widgets decide what an action
/// means (Enter submits in [`TextInput`] and breaks the line in [`TextArea`], `Home`
/// goes to the text start in one and to the line start in the other).
pub fn init(cx: &mut App) {
    let (word_left, word_right, select_word_left, select_word_right) = if cfg!(target_os = "macos")
    {
        ("alt-left", "alt-right", "alt-shift-left", "alt-shift-right")
    } else {
        (
            "ctrl-left",
            "ctrl-right",
            "ctrl-shift-left",
            "ctrl-shift-right",
        )
    };
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    let (copy, cut, paste, undo, redo) = if cfg!(target_os = "macos") {
        ("cmd-c", "cmd-x", "cmd-v", "cmd-z", "cmd-shift-z")
    } else {
        ("ctrl-c", "ctrl-x", "ctrl-v", "ctrl-z", "ctrl-shift-z")
    };

    let mut bindings = vec![
        KeyBinding::new("backspace", Backspace, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("left", Left, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("right", Right, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(word_left, WordLeft, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(word_right, WordRight, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(select_word_left, SelectWordLeft, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(select_word_right, SelectWordRight, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(select_all, SelectAll, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("home", Home, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("end", End, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("shift-home", SelectToHome, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("shift-end", SelectToEnd, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("enter", Enter, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(copy, Copy, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(cut, Cut, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(paste, Paste, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(undo, Undo, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new(redo, Redo, Some(TEXT_INPUT_CONTEXT)),
        KeyBinding::new("backspace", Backspace, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("left", Left, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("right", Right, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(word_left, WordLeft, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(word_right, WordRight, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(select_word_left, SelectWordLeft, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(select_word_right, SelectWordRight, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(select_all, SelectAll, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("home", Home, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("end", End, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("shift-home", SelectToHome, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("shift-end", SelectToEnd, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("up", Up, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("down", Down, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new("enter", Enter, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(copy, Copy, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(cut, Cut, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(paste, Paste, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(undo, Undo, Some(TEXT_AREA_CONTEXT)),
        KeyBinding::new(redo, Redo, Some(TEXT_AREA_CONTEXT)),
    ];
    // `ctrl-y` is the Windows/Linux redo twin of `ctrl-shift-z`.
    if !cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("ctrl-y", Redo, Some(TEXT_INPUT_CONTEXT)));
        bindings.push(KeyBinding::new("ctrl-y", Redo, Some(TEXT_AREA_CONTEXT)));
    }
    cx.bind_keys(bindings);
}

#[cfg(test)]
#[path = "focus_ring_tests.rs"]
mod focus_ring_tests;
