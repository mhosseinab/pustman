//! The one formatter pair every pane shows an exchange time or a response size through:
//! the Console rows, the response status line and the History rows all render a duration
//! or a byte count in the same form, so the same measurement never appears in two shapes
//! (#134).
//!
//! A missing measurement is the caller's own to present — a blank in a Console row, the
//! metric's "—" in the status line, a dropped part in the History summary line — so these
//! take the measured value and nothing else.

use std::time::Duration;

/// A duration as the product shows it: milliseconds under a second, seconds above one,
/// minutes and seconds for anything over a minute.
pub fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{:.0} ms", duration.as_secs_f64() * 1000.0)
    } else if duration < Duration::from_secs(60) {
        format!("{:.2} s", duration.as_secs_f64())
    } else {
        format!(
            "{} m {:02} s",
            duration.as_secs() / 60,
            duration.as_secs() % 60
        )
    }
}

/// A byte count as the product shows it: 1024-based `B`, `KB`, `MB`, `GB` —
/// [`format_duration`]'s size twin.
pub fn format_size(bytes: u64) -> String {
    const UNIT: f64 = 1024.0;
    // Display only: the loss of precision past 2^53 bytes cannot show in a size string.
    let bytes = bytes as f64;
    if bytes < UNIT {
        format!("{bytes:.0} B")
    } else if bytes < UNIT * UNIT {
        format!("{:.2} KB", bytes / UNIT)
    } else if bytes < UNIT * UNIT * UNIT {
        format!("{:.2} MB", bytes / UNIT / UNIT)
    } else {
        format!("{:.2} GB", bytes / UNIT / UNIT / UNIT)
    }
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
