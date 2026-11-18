//! Classifying a template's placeholders for editor highlighting (PRD §7 REQ-VAR-03).

use api_model::VariableKind;
use std::ops::Range;

use crate::dynamic_values::DynamicValues;
use crate::scope::ScopeStack;
use crate::template::placeholders;

/// How the editor draws one placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanState {
    /// It resolves: a dynamic variable, or a scoped variable with a value.
    Resolved,
    /// Nothing resolves it — unknown, or bound only by disabled variables — so it stays
    /// literal at send time.
    Unresolved,
    /// A secret: the value is masked wherever it appears, set or not (REQ-VAR-03).
    Secret,
}

/// One `{{name}}` placeholder of a template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableSpan {
    /// Byte range of the whole `{{name}}` token, braces included.
    pub range: Range<usize>,
    /// The name inside the braces, verbatim.
    pub name: String,
    /// How the editor draws it.
    pub state: SpanState,
}

/// Classify every placeholder of `template`, left to right, without resolving anything.
///
/// This reads the template the user is editing; a `{{name}}` inside some variable's
/// value gets no span of its own. A name that is a secret is [`SpanState::Secret`]
/// whether or not a value is set — highlighting masks secrets, while [`crate::resolve`]
/// reports an unset one as unresolved so nothing sends an empty string.
pub fn spans(template: &str, scopes: &ScopeStack) -> Vec<VariableSpan> {
    placeholders(template)
        .into_iter()
        .map(|placeholder| VariableSpan {
            range: placeholder.range,
            name: placeholder.name.to_string(),
            state: state_of(placeholder.name, scopes),
        })
        .collect()
}

/// How `name` is drawn.
fn state_of(name: &str, scopes: &ScopeStack) -> SpanState {
    if DynamicValues::is_dynamic_name(name) {
        return SpanState::Resolved;
    }
    match scopes.find(name) {
        Some(binding) if binding.variable.kind == VariableKind::Secret => SpanState::Secret,
        Some(binding) if binding.value.is_some() => SpanState::Resolved,
        _ => SpanState::Unresolved,
    }
}

#[cfg(test)]
#[path = "spans_tests.rs"]
mod tests;
