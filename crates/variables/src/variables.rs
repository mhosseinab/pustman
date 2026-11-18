//! `variables` — `{{name}}` resolution for every request field (PRD §7).
//!
//! A template such as `GET {{host}}/{{path}}` is resolved at send time against a
//! [`ScopeStack`] of scoped [`api_model::Variable`]s plus one [`DynamicValues`] of
//! generated names. [`resolve`] answers three questions in one pass: what the text
//! becomes ([`Resolved::text`]), which names nothing resolves — kept literal and listed
//! for the pre-send warning of REQ-VAR-07 — and which secrets were substituted
//! ([`Resolved::secrets_used`]), since their values are now in the text.
//!
//! # Scopes
//!
//! Precedence is narrowest wins: request-local, then the data slot, then environment,
//! collection and global (REQ-VAR-02). The data scope exists in the resolver but is
//! unused in v1 — there is no runner. Disabled variables are ignored, and a name's
//! value is the local value the caller supplied (the local store, or the OS keychain
//! for a secret) whenever one is set, else the shared value a file holds
//! (REQ-VAR-SHARE-01). A **secret** only ever takes the caller's value: a workspace
//! file records no secret value to take (REQ-VAR-SHARE-02).
//!
//! # Dynamic values
//!
//! `$guid`, `$timestamp`, `$isoTimestamp`, `$randomInt` and `$randomUUID` are generated
//! once per send, so every occurrence of a name in one request carries one value
//! (REQ-VAR-05). One [`DynamicValues`] is one send.
//!
//! # Depth, cycles and highlighting
//!
//! Values may reference other variables; those references are followed ten levels down
//! and a loop reports the names that form it ([`ResolveError`], REQ-VAR-06).
//! [`spans`] classifies each placeholder of the template being edited — resolved,
//! unresolved or secret — for highlighting and masking (REQ-VAR-03), without resolving
//! anything.
//!
//! Resolved text can carry secret values, so the types here keep them out of `Debug`
//! output the way `api_model::Variable` does.

mod dynamic_values;
mod resolve;
mod scope;
mod spans;
mod template;
#[cfg(test)]
mod test_support;

pub use dynamic_values::DynamicValues;
pub use resolve::{ResolveError, Resolved, resolve};
pub use scope::{Binding, Scope, ScopeKind, ScopeStack};
pub use spans::{SpanState, VariableSpan, spans};
