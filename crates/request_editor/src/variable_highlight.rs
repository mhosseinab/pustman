//! `{{variable}}` highlighting in the request fields (PRD REQ-VAR-03): every
//! placeholder a field holds paints green when it resolves, red when nothing resolves
//! it, and masked when it names a secret — recomputed when the field's text or the
//! scopes change.
//!
//! The spans come from [`variables::spans`] (never a re-parse of `{{…}}`), and their
//! byte ranges are the placeholders' ranges in the text the field holds: every field
//! edits the raw template — URL, cells, inputs and the raw body alike — so no field
//! needs a range mapping.
//!
//! `Decision:` "masked" is a distinct style, not hidden characters — highlights
//! cannot replace the text they describe — so a secret paints as muted text on a
//! mask-like block (`text_muted` on `element_background`, a contrast-tested pair of
//! existing tokens; no token is added). The hover that shows a value (masked, with a
//! reveal toggle) is S33c's work, not this module's.
//!
//! `Decision:` nothing calls [`RequestEditor::set_scopes`] yet — the environment
//! picker (S34) owns the active scopes and wires them here. Until then the stack is
//! empty and every name is unresolved, which is the truth: no scope resolves them. A
//! whole-request replacement (the cURL import's `replace_request`) rebuilds the field
//! entities with the empty default, so the `set_scopes` wiring must push the scopes
//! again after a replacement.
//!
//! `Decision:` the send path resolves against its own [`ScopeStack`] for now (`send`,
//! S33d's file) — highlighting is display-only and never feeds a send.
//!

use std::sync::Arc;

use gpui::{App, Context, Entity, HighlightStyle};
use text_input::{Highlight, TextArea, TextInput};
use ui::prelude::ActiveTheme;
use variables::{ScopeStack, SpanState, spans};

use crate::RequestEditor;
use crate::key_value_table::{KeyValueTable, RowField};

/// The highlight spans for a field's `text` against `scopes`: one span per
/// placeholder, styled by its [`SpanState`] (REQ-VAR-03).
fn highlights_for(text: &str, scopes: &ScopeStack, cx: &App) -> Vec<Highlight> {
    spans(text, scopes)
        .into_iter()
        .map(|span| (span.range, style_for(span.state, cx)))
        .collect()
}

/// How one placeholder paints: resolved green, unresolved red, secret masked — every
/// color a theme token (AGENTS.md › GPUI).
fn style_for(state: SpanState, cx: &App) -> HighlightStyle {
    let colors = cx.theme().colors();
    match state {
        SpanState::Resolved => HighlightStyle {
            color: Some(colors.variable_resolved),
            ..HighlightStyle::default()
        },
        SpanState::Unresolved => HighlightStyle {
            color: Some(colors.variable_unresolved),
            ..HighlightStyle::default()
        },
        SpanState::Secret => HighlightStyle {
            color: Some(colors.text_muted),
            background_color: Some(colors.element_background),
            ..HighlightStyle::default()
        },
    }
}

/// Re-highlight one single-line field against `scopes`.
pub fn apply_to_input(input: &Entity<TextInput>, scopes: &ScopeStack, cx: &mut App) {
    input.update(cx, |input, cx| {
        let highlights = highlights_for(&input.text(), scopes, cx);
        input.set_highlights(highlights, cx);
    });
}

/// Re-highlight one multi-line field against `scopes`.
pub fn apply_to_area(area: &Entity<TextArea>, scopes: &ScopeStack, cx: &mut App) {
    area.update(cx, |area, cx| {
        let highlights = highlights_for(&area.text(), scopes, cx);
        area.set_highlights(highlights, cx);
    });
}

/// Re-highlight the key and value cells of a key/value table against `scopes` — the
/// columns a params or headers row sends. `Decision:` description cells stay plain: a
/// description is documentation, never part of a request (REQ-HTTP-02).
pub fn apply_to_table(table: &Entity<KeyValueTable>, scopes: &ScopeStack, cx: &mut App) {
    for (field, input) in table.read(cx).cell_inputs() {
        if matches!(field, RowField::Key | RowField::Value) {
            apply_to_input(&input, scopes, cx);
        }
    }
}

impl RequestEditor {
    /// The scopes every request field highlights its `{{variables}}` against
    /// (REQ-VAR-03) — see the module docs for who calls this and when.
    pub fn set_scopes(&mut self, scopes: ScopeStack, cx: &mut Context<Self>) {
        self.scopes = Arc::new(scopes);
        self.refresh_highlights(cx);
    }

    /// Push the current scopes into every field; each one re-highlights what it
    /// edits. Never called from `render` — highlights are computed when state
    /// changes (AGENTS.md › GPUI).
    fn refresh_highlights(&self, cx: &mut Context<Self>) {
        let scopes = self.scopes.clone();
        self.url_bar
            .update(cx, |bar, cx| bar.set_scopes(scopes.clone(), cx));
        self.params
            .update(cx, |params, cx| params.set_scopes(scopes.clone(), cx));
        self.headers
            .update(cx, |headers, cx| headers.set_scopes(scopes.clone(), cx));
        self.body_tab
            .update(cx, |tab, cx| tab.set_scopes(scopes.clone(), cx));
        self.auth_tab
            .update(cx, |tab, cx| tab.set_scopes(scopes.clone(), cx));
    }
}

#[cfg(test)]
#[path = "variable_highlight_tests.rs"]
mod tests;
