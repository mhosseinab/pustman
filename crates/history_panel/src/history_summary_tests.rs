//! The History row's meta text (#134): a stored duration is shown in the same form as the
//! Console rows and the response status line show one — the shared formatter pair in `ui`
//! — not as a bare "{ms} ms" join that grows "1500 ms" where the other panes say
//! "1.50 s".

use storage::HistoryEntry;
use time::OffsetDateTime;

use super::entry_summary;

/// One history entry carrying `status` and `duration_ms`, everything else unremarkable.
fn entry(status: Option<i64>, duration_ms: Option<i64>) -> HistoryEntry {
    HistoryEntry {
        id: Some(1),
        workspace_id: "workspace-1".to_owned(),
        created_at: OffsetDateTime::from_unix_timestamp(0)
            .unwrap_or_else(|error| panic!("the epoch is a valid timestamp: {error}")),
        protocol: "http".to_owned(),
        method: "GET".to_owned(),
        target: "https://api.test/users".to_owned(),
        status,
        duration_ms,
        request_json: serde_json::json!({}),
        response_summary_json: None,
        response_body_prefix: None,
    }
}

#[test]
fn history_meta_takes_the_shared_duration_shape() {
    assert_eq!(
        entry_summary(&entry(Some(200), Some(184))),
        "200 · 184 ms",
        "milliseconds under a second"
    );
    assert_eq!(
        entry_summary(&entry(Some(200), Some(1500))),
        "200 · 1.50 s",
        "seconds above one, two decimals — the shape the Console rows and the response \
         status line show"
    );
}
