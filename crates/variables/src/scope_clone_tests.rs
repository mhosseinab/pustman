//! Cloning a [`ScopeStack`] — the one copy a send takes (#197). Both
//! `http_engine::prepare` and the request editor's send hand-copied the stack before
//! `Clone` existed; a clone must carry everything resolution can observe.

use std::collections::BTreeMap;

use crate::scope::{ScopeKind, ScopeStack};
use crate::test_support::{secret, variable};

#[test]
fn test_a_cloned_stack_resolves_exactly_like_the_original() {
    let mut scopes = ScopeStack::with_local_values(BTreeMap::from([(
        "token".to_string(),
        "token-REDACTED-example".to_string(),
    )]));
    scopes.push(ScopeKind::Global, vec![variable("host", "global.example")]);
    scopes.push(
        ScopeKind::Environment,
        vec![secret("token"), variable("host", "env.example")],
    );

    let cloned = scopes.clone();

    assert_eq!(
        cloned.find("host").and_then(|binding| binding.value),
        Some("env.example")
    );
    // A secret resolves only from the caller's local value (REQ-VAR-SHARE-02).
    assert_eq!(
        cloned.find("token").and_then(|binding| binding.value),
        Some("token-REDACTED-example")
    );
    let kinds = |stack: &ScopeStack| {
        stack
            .scopes()
            .iter()
            .map(|scope| scope.kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds(&cloned), kinds(&scopes));
}

#[test]
fn test_a_cloned_stack_keeps_a_local_value_no_scope_declares() {
    // A local value can arrive before the variable that names it; the hand-copies
    // dropped such values, and the clone must not (#197).
    let scopes = ScopeStack::with_local_values(BTreeMap::from([(
        "token".to_string(),
        "token-REDACTED-example".to_string(),
    )]));

    let cloned = scopes.clone();

    assert_eq!(cloned.local_value("token"), Some("token-REDACTED-example"));
}

#[test]
fn test_changing_a_cloned_stack_leaves_the_original_alone() {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Global, vec![variable("host", "global.example")]);

    let mut cloned = scopes.clone();
    cloned.push(
        ScopeKind::RequestLocal,
        vec![variable("host", "local.example")],
    );

    assert_eq!(
        scopes.find("host").and_then(|binding| binding.value),
        Some("global.example")
    );
    assert_eq!(
        cloned.find("host").and_then(|binding| binding.value),
        Some("local.example")
    );
}
