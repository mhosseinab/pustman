//! The megabyte unit behind `request.response_pretty_limit_mb` (#183): the unit lives
//! in [`crate::large_body`] and this file pins its value. Wired at the crate root, not
//! inside `large_body`, on purpose: only a non-descendant can tell a `pub(crate)` export
//! from a private constant, and the point of the export is that no module re-declares
//! its own copy. (`large_body_tests.rs` is frozen and keeps its literal copy; this pin
//! keeps that copy honest.)

use crate::large_body::{MEGABYTE, exceeds_pretty_limit};

/// `MEGABYTE` stays 1024 × 1024 bytes — the value `large_body_tests.rs` (read-only)
/// hard-codes for itself, so the two copies cannot drift apart.
#[test]
fn megabyte_unit_stays_1024_squared() {
    assert_eq!(
        MEGABYTE,
        1024 * 1024,
        "the pretty limit counts in 1024 × 1024-byte megabytes"
    );
}

/// The pretty-limit comparison counts in that same unit: exactly one MB is not over a
/// 1 MB limit, one byte more is.
#[test]
fn pretty_limit_counts_in_megabytes() {
    assert!(!exceeds_pretty_limit(MEGABYTE, 1));
    assert!(exceeds_pretty_limit(MEGABYTE + 1, 1));
}
