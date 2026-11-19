//! The undo model behind [`crate::input::TextInput`]: every text mutation becomes a
//! [`Transaction`] that records the replaced byte range, both sides' text and the
//! selection on either side — enough to put the field back the way it was in *both*
//! directions. A bounded pair of stacks ([`UndoStack`]) moves transactions between
//! undo and redo, and coalesces consecutive single-grapheme typing into one so a
//! word undoes as one step.
//!
//! `Decision:` this is a plain type with no GPUI in it: the undo model is buffer
//! arithmetic and stack discipline, and keeping it free of entities and windows makes
//! it testable on its own and keeps the GPUI surface in [`crate::input`]. It is shaped
//! after the *idea* of Zed's editor transactions (GPL-3.0) — never its code.
//!
//! `Decision:` the stack is bounded to [`MAX_TRANSACTIONS`] transactions; when a new
//! edit arrives at the bound the oldest transaction is dropped, so a long session
//! never grows the history without limit and undo degrades to "the recent past".
//!
//! `Decision:` coalescing breaks exactly where the step calls for — cursor motion or
//! selection changes, a deletion after an insertion (or the reverse), paste and cut,
//! undo/redo, `set_text` — and nowhere else. Whitespace/word boundaries do *not* break
//! a run and there is no time-based break (KISS: the pinned list is exhaustive, and a
//! clock would add a dependency on GPUI timing to a plain type for no required
//! behavior).

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// How many transactions the stacks keep. See the module docs for the decision.
pub const MAX_TRANSACTIONS: usize = 1000;

/// A selection snapshot: the byte range it covers and which end the caret is at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The selected byte range; an empty range is a caret.
    pub range: Range<usize>,
    /// Whether the caret is at the range's start — the selection was made backwards.
    pub reversed: bool,
}

impl Selection {
    /// A caret — an empty selection — at `offset`.
    pub fn caret(offset: usize) -> Self {
        Self {
            range: offset..offset,
            reversed: false,
        }
    }
}

/// One reversible edit: the byte range it replaced (in the text as it stood before the
/// edit), both sides' text, and the selection on either side of it. Undo replaces the
/// span [`Self::new_text_range`] covers with `old_text` and restores `selection_before`;
/// redo replaces `replaced_range` with `new_text` and restores `selection_after`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    /// The replaced byte range, in pre-edit offsets.
    pub replaced_range: Range<usize>,
    /// The text the edit replaced.
    pub old_text: String,
    /// The text the edit put in its place.
    pub new_text: String,
    /// The selection before the edit, in pre-edit offsets.
    pub selection_before: Selection,
    /// The selection after the edit, in post-edit offsets.
    pub selection_after: Selection,
}

impl Transaction {
    /// The span the new text occupies after the edit — what undo replaces with
    /// `old_text`.
    pub fn new_text_range(&self) -> Range<usize> {
        self.replaced_range.start..self.replaced_range.start + self.new_text.len()
    }

    /// Merge `range`/`text` — an edit of the text as it stands after this transaction —
    /// into this transaction, so the two undo and redo as one.
    ///
    /// `range` must fall inside the span [`Self::new_text_range`] covers; the callers
    /// that merge (typing at the caret, an IME replacing its composing text) guarantee
    /// that. Offsets are clamped to the span, which lands them on char boundaries of
    /// `new_text` — a mid-character merge can never panic or split a character.
    pub fn merge_next(&mut self, range: Range<usize>, text: &str, selection_after: Selection) {
        let start = range
            .start
            .saturating_sub(self.replaced_range.start)
            .min(self.new_text.len());
        let end = range
            .end
            .saturating_sub(self.replaced_range.start)
            .clamp(start, self.new_text.len());
        self.new_text.replace_range(start..end, text);
        self.selection_after = selection_after;
    }
}

/// How [`crate::input::TextInput::edit`] records an edit in the [`UndoStack`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    /// User typing: a single-grapheme insertion at the caret joins the open typing run,
    /// anything else is a transaction of its own that opens (or closes) a run.
    Typing,
    /// A transaction of its own: paste, cut, deletion, …
    Atomic,
    /// An IME composition update: merges into the composition in progress instead of
    /// becoming a transaction — the composition is recorded once, when it commits.
    Composition,
    /// Not recorded at all: undo/redo move existing transactions, `set_text` replaces
    /// them.
    None,
}

/// The undo/redo stacks, the IME composition being accumulated, and whether the typing
/// run is open for coalescing.
pub struct UndoStack {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    composition: Option<Transaction>,
    typing_run_open: bool,
}

impl UndoStack {
    /// An empty stack: nothing to undo, nothing to redo.
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            composition: None,
            typing_run_open: false,
        }
    }

    /// Record `transaction` the way `record` says. Any recorded edit clears the redo
    /// stack — redo covers only edits that were undone, not edits made after them.
    pub fn record(&mut self, transaction: Transaction, record: Record) {
        match record {
            Record::None => {}
            Record::Composition => {
                self.redo.clear();
                self.typing_run_open = false;
                match &mut self.composition {
                    Some(composition) => composition.merge_next(
                        transaction.replaced_range,
                        &transaction.new_text,
                        transaction.selection_after,
                    ),
                    None => self.composition = Some(transaction),
                }
            }
            Record::Typing => {
                self.redo.clear();
                self.finish_composition();
                let continues_run = self.typing_run_open
                    && self
                        .undo
                        .last()
                        .is_some_and(|previous| continues_typing_run(previous, &transaction));
                if continues_run {
                    if let Some(previous) = self.undo.last_mut() {
                        previous.merge_next(
                            transaction.replaced_range,
                            &transaction.new_text,
                            transaction.selection_after,
                        );
                    }
                } else {
                    self.typing_run_open = is_single_grapheme_insertion(&transaction);
                    self.push(transaction);
                }
            }
            Record::Atomic => {
                self.redo.clear();
                self.finish_composition();
                self.typing_run_open = false;
                self.push(transaction);
            }
        }
    }

    /// Undo the most recent transaction: move it to the redo stack and hand it back so
    /// the caller can apply its inverse. `None` — nothing to undo.
    pub fn undo(&mut self) -> Option<Transaction> {
        self.break_coalescing();
        let transaction = self.undo.pop()?;
        self.redo.push(transaction.clone());
        Some(transaction)
    }

    /// Redo the most recently undone transaction: move it back to the undo stack and
    /// hand it back so the caller can apply it. `None` — nothing to redo.
    pub fn redo(&mut self) -> Option<Transaction> {
        self.break_coalescing();
        let transaction = self.redo.pop()?;
        self.push(transaction.clone());
        Some(transaction)
    }

    /// End the open typing run and record any IME composition in progress: the next
    /// edit starts a new transaction instead of merging into the previous one.
    pub fn break_coalescing(&mut self) {
        self.finish_composition();
        self.typing_run_open = false;
    }

    /// Record the IME composition in progress as one transaction. A composition's
    /// commit folds into the composition first (see [`Record::Composition`]), so this
    /// is for compositions the platform ends without a commit — and it is harmless
    /// when no composition is open.
    pub fn finish_composition(&mut self) {
        if let Some(composition) = self.composition.take() {
            self.typing_run_open = false;
            self.push(composition);
        }
    }

    /// Whether an IME composition is being accumulated — its next edit merges into it
    /// instead of recording a transaction of its own.
    pub fn has_composition(&self) -> bool {
        self.composition.is_some()
    }

    /// Drop every transaction: `set_text` treats programmatic replacement as not being
    /// a user edit, so there is nothing to undo into.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.composition = None;
        self.typing_run_open = false;
    }

    /// Push onto the undo stack, dropping the oldest transaction at the bound.
    fn push(&mut self, transaction: Transaction) {
        if self.undo.len() >= MAX_TRANSACTIONS {
            self.undo.remove(0);
        }
        self.undo.push(transaction);
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `next` continues `previous`'s typing run: one grapheme typed at exactly the
/// point where the run left the caret, with nothing in between.
fn continues_typing_run(previous: &Transaction, next: &Transaction) -> bool {
    next.replaced_range.is_empty()
        && next.old_text.is_empty()
        && next.replaced_range.start == previous.new_text_range().end
        && is_single_grapheme_insertion(next)
}

/// Whether `transaction` inserts exactly one grapheme cluster at an empty range.
fn is_single_grapheme_insertion(transaction: &Transaction) -> bool {
    transaction.replaced_range.is_empty()
        && transaction.old_text.is_empty()
        && transaction.new_text.graphemes(true).count() == 1
}

#[cfg(test)]
#[path = "editing_tests.rs"]
mod tests;
