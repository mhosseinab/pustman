use std::collections::BTreeMap;

use crate::dynamic_values::DynamicValues;
use crate::resolve::{ResolveError, resolve};
use crate::scope::{ScopeKind, ScopeStack};
use crate::test_support::{secret, variable};
use api_model::Variable;

#[test]
fn unresolved_kept_literal_and_listed() {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![variable("host", "example.test")],
    );
    let mut dynamics = DynamicValues::new();

    let resolved = resolve(
        "GET https://{{host}}/{{missing}}/{{missing}}/{{also_missing}}",
        &scopes,
        &mut dynamics,
    )
    .unwrap();

    // Names nothing resolves stay exactly as written (REQ-VAR-07 keeps sending open).
    assert_eq!(
        resolved.text,
        "GET https://example.test/{{missing}}/{{missing}}/{{also_missing}}"
    );
    assert_eq!(resolved.unresolved, vec!["missing", "also_missing"]);
    assert!(resolved.secrets_used.is_empty());
}

#[test]
fn cycle_is_reported_with_names() {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("a", "{{b}}"),
            variable("b", "{{c}}"),
            variable("c", "{{a}}"),
        ],
    );
    let mut dynamics = DynamicValues::new();

    let error = resolve("{{a}}", &scopes, &mut dynamics).unwrap_err();

    // The error names the cycle as entered: a → b → c → a (REQ-VAR-06).
    assert_eq!(
        error,
        ResolveError::Cycle(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "a".to_string()
        ])
    );
}

#[test]
fn depth_over_ten_fails() {
    // n0 → n1 → … → "end": a chain of `length` variables nests `length` references deep.
    fn chain(length: usize) -> Vec<Variable> {
        (0..length)
            .map(|index| {
                let value = if index + 1 == length {
                    "end".to_string()
                } else {
                    format!("{{{{n{}}}}}", index + 1)
                };
                variable(&format!("n{index}"), &value)
            })
            .collect()
    }

    let mut dynamics = DynamicValues::new();

    let mut ten_deep = ScopeStack::new();
    ten_deep.push(ScopeKind::Environment, chain(10));
    let resolved = resolve("{{n0}}", &ten_deep, &mut dynamics).unwrap();
    assert_eq!(
        resolved.text, "end",
        "ten nested references resolve (REQ-VAR-06)"
    );

    let mut eleven_deep = ScopeStack::new();
    eleven_deep.push(ScopeKind::Environment, chain(11));
    let error = resolve("{{n0}}", &eleven_deep, &mut dynamics).unwrap_err();
    assert_eq!(
        error,
        ResolveError::TooDeep,
        "an eleventh level is over the depth limit of ten"
    );
}

#[test]
fn secret_value_comes_from_local_values_and_is_tracked() {
    let mut scopes = ScopeStack::with_local_values(BTreeMap::from([(
        "api_key".to_string(),
        "token-REDACTED-example".to_string(),
    )]));
    scopes.push(
        ScopeKind::Environment,
        vec![
            // A file carrying a value must not be able to set a secret.
            Variable {
                value: Some("value-from-the-file".to_string()),
                ..secret("api_key")
            },
            secret("unset_secret"),
        ],
    );
    let mut dynamics = DynamicValues::new();

    let resolved = resolve(
        "Bearer {{api_key}} {{api_key}} {{unset_secret}}",
        &scopes,
        &mut dynamics,
    )
    .unwrap();

    assert_eq!(
        resolved.text,
        "Bearer token-REDACTED-example token-REDACTED-example {{unset_secret}}"
    );
    assert!(
        !resolved.text.contains("value-from-the-file"),
        "a secret must never resolve from the file (REQ-VAR-SHARE-02)"
    );
    assert_eq!(resolved.secrets_used, vec!["api_key"]);
    // An unset secret is unresolved — never an empty string sent silently.
    assert_eq!(resolved.unresolved, vec!["unset_secret"]);
}
