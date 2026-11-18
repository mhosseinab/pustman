use std::collections::BTreeMap;

use crate::scope::{ScopeKind, ScopeStack};
use crate::spans::{SpanState, spans};
use crate::test_support::{secret, variable};

#[test]
fn spans_mark_secret_and_unresolved() {
    let mut scopes = ScopeStack::with_local_values(BTreeMap::from([(
        "api_key".to_string(),
        "token-REDACTED-example".to_string(),
    )]));
    scopes.push(
        ScopeKind::Environment,
        vec![variable("host", "example.test"), secret("api_key")],
    );

    let template = "{{host}} {{api_key}} {{missing}} {{$guid}}";
    let found = spans(template, &scopes);

    let states: Vec<(&str, SpanState)> = found
        .iter()
        .map(|span| (span.name.as_str(), span.state))
        .collect();
    assert_eq!(
        states,
        vec![
            ("host", SpanState::Resolved),
            ("api_key", SpanState::Secret),
            ("missing", SpanState::Unresolved),
            ("$guid", SpanState::Resolved),
        ]
    );

    // Each range is the whole placeholder, braces included (REQ-VAR-03 highlights it).
    let covered: Vec<&str> = found
        .iter()
        .map(|span| &template[span.range.clone()])
        .collect();
    assert_eq!(
        covered,
        vec!["{{host}}", "{{api_key}}", "{{missing}}", "{{$guid}}"]
    );
}
