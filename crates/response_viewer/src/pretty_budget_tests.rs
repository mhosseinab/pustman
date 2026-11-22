//! The output budget's refusal is asserted in its own test (#138):
//! `deeply_nested_unclosed_markup_stays_bounded` in `pretty_tests.rs` (read-only) only
//! asserts its `if let Ok(…)` branch, which never runs — the formatter always refuses
//! that hostile body — so the fallback itself is pinned here.

use super::{FormatError, Formatter};

/// The body the frozen boundedness test builds: a run of unclosed tags that indents up to
/// the cap and would dwarf the input if formatted. The formatter refuses it with
/// [`FormatError::TooLarge`], and the view then shows the Raw text with its inline note.
#[test]
fn deeply_nested_unclosed_markup_is_refused_as_too_large() {
    let body = "<a>".repeat(30_000);
    match Formatter::Xml.format(body.as_bytes()) {
        Err(FormatError::TooLarge) => {}
        Err(error) => panic!("30 000 unclosed tags must be refused as TooLarge, got {error}"),
        Ok(formatted) => panic!(
            "30 000 unclosed tags must be refused as TooLarge, not formatted to {} bytes",
            formatted.len()
        ),
    }
}
