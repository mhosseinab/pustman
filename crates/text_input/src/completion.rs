//! The completion trigger behind a field's autocomplete popover (PRD REQ-VAR-04): the
//! `{{` at the caret and the partial name typed after it, the byte range a chosen name
//! replaces, and the insert that replaces it — through [`EditState::edit`], the one path
//! every text mutation takes, so a completion is one [`TextInputEvent::Changed`] and
//! undoes like any edit.
//!
//! `Decision:` the trigger is `{{` immediately before the caret plus the name
//! characters typed after it, and nothing else: in `{{a {{b` the nearest `{{` wins, in
//! `{{a b` there is no trigger (a partial name holds no space), and a selection — not a
//! caret — opens nothing. The hook reads text and offsets only: what the popover shows
//! and which name is chosen belong to the caller (`request_editor`'s
//! `variable_autocomplete`), and so does where the popover is drawn. The caller maps
//! [`Completion::anchor_offset`] — the trigger's start — through the field's geometry.

use std::ops::Range;

use gpui::{Context, EventEmitter, Window};

use crate::editing::{Record, Selection};
use crate::input::{EditState, TextInputEvent};

/// The closing braces a completed placeholder carries.
const CLOSE: &str = "}}";

/// A completion trigger at the caret: the partial name typed after `{{`, the byte range
/// a chosen name replaces, and where the popover anchors.
pub struct Completion {
    query: String,
    replace_range: Range<usize>,
    anchor_offset: usize,
}

impl Completion {
    /// The partial name typed after `{{` — what the popover filters its names on.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The byte range the insert replaces: the partial name's range, between the `{{`
    /// and the caret.
    pub fn replace_range(&self) -> &Range<usize> {
        &self.replace_range
    }

    /// The byte offset the popover anchors at: the `{{` trigger's start.
    pub fn anchor_offset(&self) -> usize {
        self.anchor_offset
    }
}

/// The completion trigger at `state`'s caret, if one is open.
pub fn completion_at(state: &EditState) -> Option<Completion> {
    if !state.selected_range().is_empty() {
        return None;
    }
    let caret = state.cursor_offset();
    let text = state.text();
    let before = text.get(..caret)?;
    let trigger = before.rfind("{{")?;
    let query_start = trigger + "{{".len();
    let query = &before[query_start..];
    if !query.chars().all(is_name_char) {
        return None;
    }
    Some(Completion {
        query: query.to_string(),
        replace_range: query_start..caret,
        anchor_offset: trigger,
    })
}

/// Replace `completion`'s range with `name}}` — or with just `name` when `}}` already
/// follows the caret, so completing never writes a second pair of closing braces. The
/// caret lands after the placeholder's `}}` either way.
///
/// `Decision:` the insert is [`Record::Atomic`], like a paste: one undoable transaction
/// of its own rather than a typing run the next keystroke would coalesce into.
pub fn insert<E: EventEmitter<TextInputEvent>>(
    state: &mut EditState,
    completion: &Completion,
    name: &str,
    window: &mut Window,
    cx: &mut Context<E>,
) {
    // The text may have moved since the trigger was read: clamp the range to it — the
    // `HighlightSet::set` contract — instead of panicking on a stale range.
    let end = completion.replace_range.end.min(state.len());
    let start = completion.replace_range.start.min(end);
    let text = state.text();
    let closes = !text.get(end..).is_some_and(|rest| rest.starts_with(CLOSE));
    let inserted = if closes {
        format!("{name}{CLOSE}")
    } else {
        name.to_string()
    };
    // The caret lands after the placeholder's braces: the ones just written, or the
    // ones already sitting behind the caret.
    let caret = start + inserted.len() + if closes { 0 } else { CLOSE.len() };
    state.edit(
        start..end,
        &inserted,
        Some(Selection::caret(caret)),
        Record::Atomic,
        window,
        cx,
    );
}

/// The characters a partial name may hold: a name ends at a space, a brace or any other
/// punctuation, so `{{a b` is never a trigger.
fn is_name_char(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '.')
}
