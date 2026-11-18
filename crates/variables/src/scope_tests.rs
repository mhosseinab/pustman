use api_model::Variable;

use crate::scope::{ScopeKind, ScopeStack};
use crate::test_support::variable;

/// The value `{{host}}` resolves to through `scopes`, pushed widest first so the result
/// can only come from precedence (REQ-VAR-02).
fn host_value(scopes: &[(ScopeKind, Vec<Variable>)]) -> Option<String> {
    let mut stack = ScopeStack::new();
    for (kind, variables) in scopes {
        stack.push(*kind, variables.clone());
    }
    stack
        .find("host")
        .and_then(|binding| binding.value)
        .map(str::to_string)
}

#[test]
fn narrowest_scope_wins() {
    let global = (ScopeKind::Global, vec![variable("host", "global.example")]);
    let collection = (
        ScopeKind::Collection,
        vec![variable("host", "collection.example")],
    );
    let environment = (
        ScopeKind::Environment,
        vec![variable("host", "environment.example")],
    );
    let request_local = (
        ScopeKind::RequestLocal,
        vec![variable("host", "request.example")],
    );

    // Each list is widest-first: a narrow scope wins however the stack was built.
    assert_eq!(
        host_value(&[
            global.clone(),
            collection.clone(),
            environment.clone(),
            request_local
        ]),
        Some("request.example".to_string())
    );
    assert_eq!(
        host_value(&[global.clone(), collection.clone(), environment]),
        Some("environment.example".to_string())
    );
    assert_eq!(
        host_value(&[global.clone(), collection]),
        Some("collection.example".to_string())
    );
    assert_eq!(host_value(&[global]), Some("global.example".to_string()));
}

#[test]
fn disabled_variable_ignored() {
    let mut scopes = ScopeStack::new();
    scopes.push(ScopeKind::Global, vec![variable("host", "global.example")]);
    scopes.push(
        ScopeKind::Environment,
        vec![Variable {
            enabled: false,
            ..variable("host", "environment.example")
        }],
    );

    // The disabled environment variable does not shadow the global one.
    assert_eq!(
        scopes.find("host").and_then(|binding| binding.value),
        Some("global.example")
    );

    // A name bound only by disabled variables is not bound at all.
    scopes.push(
        ScopeKind::Collection,
        vec![Variable {
            enabled: false,
            ..variable("gone", "never")
        }],
    );
    assert!(scopes.find("gone").is_none());
}
