//! The save cadence pin (issue #102): `workspace` re-exports `SAVE_DEBOUNCE`, but
//! `crates/pustman/tests/wire_restore.rs` carries its own `2s` literal for the same
//! cadence — that frozen test cannot be edited from this lane, so this test pins the
//! value its literal duplicates. A cadence change fails here first, pointing at the
//! drift instead of letting the frozen literal silently disagree.

use std::time::Duration;

use crate::SAVE_DEBOUNCE;

/// The cadence `wire_restore.rs`'s
/// `const SAVE_DEBOUNCE: Duration = Duration::from_secs(2)` stands for: the exported
/// constant has to stay equal to it.
#[test]
fn save_debounce_is_the_two_seconds_the_frozen_wire_test_duplicates() {
    assert_eq!(
        SAVE_DEBOUNCE,
        Duration::from_secs(2),
        "crates/pustman/tests/wire_restore.rs hardcodes 2 s; the two move together"
    );
}
