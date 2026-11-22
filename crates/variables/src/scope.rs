//! Where a `{{name}}` finds its value: ordered scopes of [`Variable`]s plus the
//! caller's local values (PRD §7 REQ-VAR-02, REQ-VAR-SHARE-01/02).

use api_model::{Variable, VariableKind};
use std::collections::BTreeMap;
use std::fmt;

/// The scope a set of variables belongs to.
///
/// Variants are declared in precedence order and derive [`Ord`] in that order:
/// [`ScopeKind::RequestLocal`] is the narrowest and [`ScopeKind::Global`] the widest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeKind {
    /// Variables local to one request — a draft, not shared (narrowest).
    RequestLocal,
    /// The data scope of a runner. It exists in the resolver but is unused in v1
    /// (there is no runner, REQ-VAR-02).
    Data,
    /// The active environment.
    Environment,
    /// The request's collection, its folders' variables included.
    Collection,
    /// Globals, shared by every request (widest).
    Global,
}

/// One scope's variables, in the order the caller supplied them.
#[derive(Clone, Debug)]
pub struct Scope {
    /// Which scope this is; decides its position in the [`ScopeStack`].
    pub kind: ScopeKind,
    /// The variables of this scope, file order.
    pub variables: Vec<Variable>,
}

/// What a name resolves to: the variable that won and the value it carries.
#[derive(Clone, Copy)]
pub struct Binding<'a> {
    /// The narrowest enabled variable with this name.
    pub variable: &'a Variable,
    /// The value it resolves to: its local value when the caller supplied one, else its
    /// shared value. `None` for a secret with no local value.
    pub value: Option<&'a str>,
}

impl fmt::Debug for Binding<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Mirrors `api_model::Variable`: a secret's value never reaches Debug output.
        let value = match self.variable.kind {
            VariableKind::Secret => self.value.map(|_| "REDACTED"),
            VariableKind::Default => self.value,
        };
        f.debug_struct("Binding")
            .field("variable", &self.variable)
            .field("value", &value)
            .finish()
    }
}

/// Ordered scopes of [`Variable`]s, consulted narrowest first.
///
/// A name resolves to the first enabled variable carrying it, so a request-local
/// variable beats an environment one and so on out to globals (REQ-VAR-02). Variables
/// whose `enabled` flag is false are ignored entirely and do not shadow a wider scope.
///
/// Values live in two places. A shared value comes with the variable (what a workspace
/// file holds); a **local** value comes from the caller — the local store for shared
/// variables, the OS keychain for secrets — and wins whenever it is set
/// (REQ-VAR-SHARE-01). A secret's value only ever comes from that map, never from a
/// file, so a repository can never carry one (REQ-VAR-SHARE-02).
///
/// A stack is [`Clone`]: a send takes its own copy while the editor keeps sharing the
/// original, and the clone carries every scope and local value (#197).
#[derive(Clone)]
pub struct ScopeStack {
    /// Scopes in precedence order, narrowest first.
    scopes: Vec<Scope>,
    /// Per-name values supplied by the caller, keyed by variable name. This map is the
    /// only place a secret value may live, so its Debug output shows names alone.
    local_values: BTreeMap<String, String>,
}

impl ScopeStack {
    /// An empty stack: every name is unresolved.
    pub fn new() -> Self {
        Self {
            scopes: Vec::new(),
            local_values: BTreeMap::new(),
        }
    }

    /// A stack whose variables read their value from `local_values` first.
    pub fn with_local_values(local_values: BTreeMap<String, String>) -> Self {
        Self {
            scopes: Vec::new(),
            local_values,
        }
    }

    /// Add one scope's variables.
    ///
    /// The scope lands at its precedence position, so pushing globals before a
    /// request-local scope still resolves request-local first (REQ-VAR-02). Scopes of
    /// the same kind keep their push order.
    pub fn push(&mut self, kind: ScopeKind, variables: Vec<Variable>) {
        let index = self.scopes.partition_point(|scope| scope.kind <= kind);
        self.scopes.insert(index, Scope { kind, variables });
    }

    /// The scopes, narrowest first.
    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// The local value the caller supplied for `name`, if any.
    pub fn local_value(&self, name: &str) -> Option<&str> {
        self.local_values.get(name).map(String::as_str)
    }

    /// Set the local value the caller supplies for `name` — where a secret's value
    /// lives, since a file never records one (REQ-VAR-SHARE-02).
    pub fn set_local_value(&mut self, name: &str, value: &str) {
        self.local_values
            .insert(name.to_string(), value.to_string());
    }

    /// The narrowest enabled variable named `name`, with the value it resolves to.
    ///
    /// Disabled variables are ignored: a disabled name in a narrow scope does not shadow
    /// a wider one. Within one scope the first enabled variable with the name wins.
    pub fn find(&self, name: &str) -> Option<Binding<'_>> {
        let variable = self
            .scopes
            .iter()
            .flat_map(|scope| &scope.variables)
            .find(|variable| variable.enabled && variable.key == name)?;
        Some(Binding {
            variable,
            value: self.value_of(variable),
        })
    }

    /// The value a bound variable resolves to: its local value when the caller supplied
    /// one, else its shared value. A secret never resolves from the file.
    fn value_of<'a>(&'a self, variable: &'a Variable) -> Option<&'a str> {
        if let Some(local) = self.local_value(&variable.key) {
            return Some(local);
        }
        match variable.kind {
            VariableKind::Secret => None,
            VariableKind::Default => variable.value.as_deref(),
        }
    }
}

impl Default for ScopeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ScopeStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Local values can hold secrets: only their names are printed.
        f.debug_struct("ScopeStack")
            .field("scopes", &self.scopes)
            .field(
                "local_values",
                &self.local_values.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
#[path = "scope_clone_tests.rs"]
mod clone_tests;

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
