//! Copy checks for [`TransportError::suggested_fix`] (LOW #209): the advice shown next
//! to a failure must name something the user can actually do in M1, where no Environments
//! UI exists yet.

use crate::TransportError;

#[test]
fn test_invalid_url_suggested_fix_does_not_send_the_user_to_an_environments_ui() {
    let error = TransportError::InvalidUrl {
        url: "https://{{host}}/v1".to_string(),
    };

    let fix = error.suggested_fix();

    assert!(
        !fix.to_lowercase().contains("environment"),
        "M1 has no Environments UI, so the fix must not point at one: {fix}"
    );
    assert!(
        fix.contains("URL"),
        "the only thing the user can act on in M1 is the URL: {fix}"
    );
}
