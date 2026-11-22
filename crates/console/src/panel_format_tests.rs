//! The Console row's time and size text (#134): the row shows the same duration or size
//! in the same form as the response status line and the History rows — one shared
//! formatter pair in `ui`, not this pane's own. A value that the other panes render as
//! "1.23 s" or "1.50 KB" must not appear here as "1.2 s" or "1.5 KB".

use std::time::Duration;

use super::{format_size, format_time};

#[test]
fn row_time_takes_the_shared_duration_shape() {
    assert_eq!(
        format_time(Some(Duration::from_millis(184))),
        "184 ms",
        "milliseconds under a second"
    );
    assert_eq!(
        format_time(Some(Duration::from_millis(1234))),
        "1.23 s",
        "seconds above one, two decimals — the shape the response status line shows"
    );
    assert_eq!(
        format_time(Some(Duration::from_secs(65))),
        "1 m 05 s",
        "minutes and seconds past a minute"
    );
    assert_eq!(format_time(None), "", "an unmeasured time shows nothing");
}

#[test]
fn row_size_takes_the_shared_size_shape() {
    assert_eq!(format_size(Some(244)), "244 B", "bytes as themselves");
    assert_eq!(
        format_size(Some(1536)),
        "1.50 KB",
        "1024-based units, two decimals — the shape the response status line shows"
    );
    assert_eq!(format_size(None), "", "an unmeasured size shows nothing");
}
