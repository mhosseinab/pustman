use super::DynamicValues;
use crate::resolve::resolve;
use crate::scope::ScopeStack;

#[test]
fn dynamic_value_stable_within_one_send() {
    // Every name twice: one DynamicValues is one send (REQ-VAR-05).
    let template = "{{$guid}} {{$guid}} {{$timestamp}} {{$timestamp}} {{$randomInt}} \
                    {{$randomInt}} {{$randomUUID}} {{$randomUUID}} {{$isoTimestamp}} \
                    {{$isoTimestamp}}";
    let mut send = DynamicValues::new();
    let scopes = ScopeStack::new();

    let resolved = resolve(template, &scopes, &mut send).unwrap();
    let values: Vec<&str> = resolved.text.split_whitespace().collect();

    assert_eq!(values.len(), 10);
    assert_eq!(values[0], values[1], "$guid must hold one value per send");
    assert_eq!(
        values[2], values[3],
        "$timestamp must hold one value per send"
    );
    assert_eq!(
        values[4], values[5],
        "$randomInt must hold one value per send"
    );
    assert_eq!(
        values[6], values[7],
        "$randomUUID must hold one value per send"
    );
    assert_eq!(
        values[8], values[9],
        "$isoTimestamp must hold one value per send"
    );
    assert!(
        values
            .iter()
            .all(|value| !value.contains("{{") && !value.is_empty()),
        "every dynamic name must produce a value: {}",
        resolved.text
    );
    assert!(resolved.unresolved.is_empty());
    assert!(resolved.secrets_used.is_empty());

    // Memoization, not constants: the next send generates again.
    let mut next_send = DynamicValues::new();
    let fresh = resolve("{{$guid}}", &scopes, &mut next_send).unwrap();
    assert_ne!(
        fresh.text, values[0],
        "a new send must generate a new $guid"
    );

    assert!(DynamicValues::is_dynamic_name("$isoTimestamp"));
    assert!(!DynamicValues::is_dynamic_name("$randomFoo"));
}
