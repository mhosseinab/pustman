//! The secret-set helpers a bounded text window relies on: the longest kept secret, and
//! the longest prefix no secret occurrence straddles the end of. Fixture values are
//! obviously fake (`.agents/rules/tests.md`).

use crate::SecretValues;

/// The longest secret sizes the window a caller reads; refused values are not kept and
/// do not count.
#[test]
fn max_len_reports_the_longest_kept_secret() {
    let secrets = SecretValues::from_values(["xy", "four", "token-REDACTED-example"]);
    assert_eq!(secrets.max_len(), "token-REDACTED-example".len());
    assert_eq!(SecretValues::new().max_len(), 0);
}

/// A tail that is the start of a secret whose rest lies beyond the text is cut: keeping
/// it would leave the unmasked start of an occurrence in the clear.
#[test]
fn straddle_free_prefix_drops_a_tail_that_could_continue_into_a_secret() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    assert_eq!(
        secrets.straddle_free_prefix("keep token-REDACTED"),
        "keep ",
        "the secret's unmasked start is cut away"
    );
    assert_eq!(
        secrets.straddle_free_prefix("keep token-REDACTED-example"),
        "keep token-REDACTED-example",
        "a whole occurrence is no straddle and stays for `redact_text` to mask"
    );
}

/// The cut cascades: after it, the new end must not straddle either.
#[test]
fn straddle_free_prefix_trims_until_no_tail_continues_a_secret() {
    let secrets = SecretValues::from_values(["ababQ"]);
    assert_eq!(
        secrets.straddle_free_prefix("xxabababab"),
        "xx",
        "each cut can expose another tail that continues the secret"
    );
}

/// Text that ends no secret prefix is kept whole.
#[test]
fn straddle_free_prefix_keeps_text_that_ends_no_secret_prefix() {
    let secrets = SecretValues::from_values(["token-REDACTED-example"]);
    assert_eq!(
        secrets.straddle_free_prefix("plain text ending"),
        "plain text ending"
    );
    assert_eq!(secrets.straddle_free_prefix(""), "");
}
