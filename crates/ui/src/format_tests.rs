//! The shared formatter pair's shapes (#134): one duration form and one size form, pinned
//! so every pane that calls them shows the same measurement in the same text.

use std::time::Duration;

use super::{format_duration, format_size};

#[test]
fn durations_show_milliseconds_then_seconds_then_minutes() {
    assert_eq!(
        format_duration(Duration::from_millis(184)),
        "184 ms",
        "milliseconds under a second"
    );
    assert_eq!(
        format_duration(Duration::from_millis(1234)),
        "1.23 s",
        "seconds above one, two decimals"
    );
    assert_eq!(
        format_duration(Duration::from_secs(65)),
        "1 m 05 s",
        "minutes and seconds past a minute"
    );
}

#[test]
fn sizes_show_1024_based_units() {
    assert_eq!(format_size(244), "244 B", "bytes as themselves");
    assert_eq!(format_size(1536), "1.50 KB", "two decimals above a byte");
    assert_eq!(
        format_size(3 * 1024 * 1024),
        "3.00 MB",
        "megabytes as megabytes"
    );
    assert_eq!(
        format_size(2 * 1024 * 1024 * 1024),
        "2.00 GB",
        "gigabytes as gigabytes"
    );
}
