//! Turning a template into the text to send (PRD §7 REQ-VAR-01/05/06/07).

use api_model::VariableKind;
use std::fmt;

use crate::dynamic_values::DynamicValues;
use crate::scope::ScopeStack;
use crate::template::placeholders;

/// The deepest chain of nested references that still resolves: `{{a}}` whose value holds
/// `{{b}}`, whose value holds `{{c}}`, and so on, ten levels down (REQ-VAR-06).
const MAX_DEPTH: usize = 10;

/// A template after resolution.
#[derive(Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The text to send: resolved values substituted in, names nothing resolves left
    /// literal.
    pub text: String,
    /// The names that did not resolve, in first-appearance order and without
    /// duplicates. Each stays literal in `text`, so the pre-send warning can list them
    /// (REQ-VAR-07) and sending anyway remains possible.
    pub unresolved: Vec<String>,
    /// The names of the secret variables substituted into `text`, in first-appearance
    /// order and without duplicates. `text` carries their values, so this is the list of
    /// what must never be logged.
    pub secrets_used: Vec<String>,
}

impl fmt::Debug for Resolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Resolved");
        if self.secrets_used.is_empty() {
            debug.field("text", &self.text);
        } else {
            // `text` holds substituted secret values: never print them.
            debug.field("text", &"<redacted>");
        }
        debug
            .field("unresolved", &self.unresolved)
            .field("secrets_used", &self.secrets_used)
            .finish()
    }
}

/// Why a template could not be resolved.
///
/// Both variants name variables, never values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// Variables reference each other in a loop. The names are the cycle as it was
    /// entered, with the first name repeated at the end: `a → b → a`.
    #[error("variable cycle: {}", .0.join(" -> "))]
    Cycle(Vec<String>),
    /// References nest deeper than the limit of [`MAX_DEPTH`] levels.
    #[error("variable references nested deeper than {} levels", MAX_DEPTH)]
    TooDeep,
}

/// Resolve every `{{name}}` of `template` against `scopes`, generating dynamic values
/// through `dynamics` (one per send).
///
/// Names that resolve are substituted — including names inside a resolved value, which
/// expands recursively. Names that do not resolve stay as the literal `{{name}}` in
/// [`Resolved::text`] and are listed in [`Resolved::unresolved`]; a secret with no value
/// counts as unresolved, so an unset secret is never sent as an empty string
/// (REQ-VAR-SHARE-02). Substituting a secret records its name in
/// [`Resolved::secrets_used`].
///
/// # Errors
///
/// [`ResolveError::Cycle`] when variables reference each other in a loop, and
/// [`ResolveError::TooDeep`] when nested references pass the depth of ten (REQ-VAR-06).
/// Nothing is resolved in either case.
pub fn resolve(
    template: &str,
    scopes: &ScopeStack,
    dynamics: &mut DynamicValues,
) -> Result<Resolved, ResolveError> {
    let mut resolution = Resolution {
        scopes,
        dynamics,
        unresolved: Vec::new(),
        secrets_used: Vec::new(),
        visiting: Vec::new(),
    };
    let text = resolution.expand(template, 0)?;
    Ok(Resolved {
        text,
        unresolved: resolution.unresolved,
        secrets_used: resolution.secrets_used,
    })
}

/// What one [`resolve`] call carries through its recursion.
struct Resolution<'a> {
    scopes: &'a ScopeStack,
    dynamics: &'a mut DynamicValues,
    unresolved: Vec<String>,
    secrets_used: Vec<String>,
    /// The names whose values are being expanded right now, outermost first. A name that
    /// comes up again is a cycle.
    visiting: Vec<String>,
}

impl Resolution<'_> {
    /// Expand every placeholder of `text`, whose references sit `depth` levels of
    /// nesting below the template itself.
    fn expand(&mut self, text: &str, depth: usize) -> Result<String, ResolveError> {
        let mut expanded = String::with_capacity(text.len());
        let mut cursor = 0;
        for placeholder in placeholders(text) {
            expanded.push_str(&text[cursor..placeholder.range.start]);
            expanded.push_str(&self.substitute(placeholder.name, depth + 1)?);
            cursor = placeholder.range.end;
        }
        expanded.push_str(&text[cursor..]);
        Ok(expanded)
    }

    /// What one `{{name}}` becomes: a generated value, an expanded variable value, or
    /// the placeholder kept literal.
    fn substitute(&mut self, name: &str, depth: usize) -> Result<String, ResolveError> {
        if depth > MAX_DEPTH {
            return Err(ResolveError::TooDeep);
        }
        if let Some(value) = self.dynamics.value(name) {
            return Ok(value.to_string());
        }
        let Some(binding) = self.scopes.find(name) else {
            self.note_unresolved(name);
            return Ok(literal(name));
        };
        let Some(value) = binding.value else {
            // A secret with no value is unresolved like any other name — never an empty
            // string on the wire (REQ-VAR-SHARE-02).
            self.note_unresolved(name);
            return Ok(literal(name));
        };
        if let Some(start) = self
            .visiting
            .iter()
            .position(|visiting| visiting.as_str() == name)
        {
            let mut cycle = self.visiting[start..].to_vec();
            cycle.push(name.to_string());
            return Err(ResolveError::Cycle(cycle));
        }
        if binding.variable.kind == VariableKind::Secret {
            self.note_secret(name);
        }
        self.visiting.push(name.to_string());
        let expanded = self.expand(value, depth);
        self.visiting.pop();
        expanded
    }

    fn note_unresolved(&mut self, name: &str) {
        push_new(&mut self.unresolved, name);
    }

    fn note_secret(&mut self, name: &str) {
        push_new(&mut self.secrets_used, name);
    }
}

/// `{{name}}` as it stays in the text when nothing resolves it.
fn literal(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

/// Add `name` unless it is already listed: a warning names a variable once.
fn push_new(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|listed| listed.as_str() == name) {
        list.push(name.to_string());
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
