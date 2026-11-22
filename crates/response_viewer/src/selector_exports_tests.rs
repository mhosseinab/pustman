//! Pins the debug-selector exports the embedded-view tests in host crates name (#186).
//!
//! `request_editor`'s `response_split_tests.rs` and `m1_layout_fixes_tests.rs` are frozen
//! and still spell "response-body-pane" as a literal, so the exported constant must stay
//! in lockstep with what those tests look for.

// The pin: re-exporting a non-`pub` item is a compile error (E0364), so this file stops
// compiling the moment `BODY_PANE_SELECTOR` is no longer exported.
pub use crate::BODY_PANE_SELECTOR;

/// The literal the frozen `request_editor` tests use for the body pane.
const FROZEN_LITERAL: &str = "response-body-pane";

#[test]
fn test_exported_body_pane_selector_matches_the_literal_the_frozen_host_tests_use() {
    assert_eq!(BODY_PANE_SELECTOR, FROZEN_LITERAL);
}
