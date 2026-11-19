//! `text_input` — the single-line text field every screen edits through.
//!
//! Two doors in:
//! - [`input`] — the [`TextInput`] entity: typing, cursor motion, shift selection, IME
//!   through [`gpui::EntityInputHandler`], a placeholder, read-only mode, and the
//!   [`TextInputEvent`]s (`Changed`, `Submitted`) an owner subscribes to.
//! - [`init`] — called once at startup, after `ui`: it binds the editing keys the field
//!   answers to under the widget's own [`input::TEXT_INPUT_CONTEXT`].
//!
//! The buffer under it ([`buffer`]) is a gap buffer, so typing at the cursor never
//! copies the whole text; see the module docs there for the decision. Undo and redo run
//! on the transaction model in [`editing`]; copy, cut and paste go through the same
//! edit path and the platform clipboard. A multi-line `TextArea` and text highlights
//! are S20c.

pub mod buffer;
pub mod editing;
pub mod input;

pub use input::{TextInput, TextInputEvent};

use gpui::{App, KeyBinding};

use input::{
    Backspace, Copy, Cut, Delete, End, Enter, Home, Left, Paste, Redo, Right, SelectAll,
    SelectLeft, SelectRight, SelectToEnd, SelectToHome, SelectWordLeft, SelectWordRight,
    TEXT_INPUT_CONTEXT, Undo, WordLeft, WordRight,
};

/// Set the widget up: bind the editing keys every [`TextInput`] answers to.
///
/// `Decision:` these bindings belong to the widget, not to `assets/keymaps/*` — they are
/// the editing semantics of a text field (Zed binds its `Editor` the same way), not PRD
/// actions a user remaps, so `assets/keymaps/*` (S21) is not edited here.
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
    ];
    // `ctrl-y` is the Windows/Linux redo twin of `ctrl-shift-z`.
    if !cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("ctrl-y", Redo, Some(TEXT_INPUT_CONTEXT)));
    }
    cx.bind_keys(bindings);
}
