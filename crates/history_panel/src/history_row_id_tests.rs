//! Rows keep the identity the store gives them and never share one (#111): a row the
//! store left without an id — unreachable today, since stored rows always carry ids —
//! gets its own fallback element id, where every such row used to mint the one shared
//! `history-entry` (and an index past the rows snapshot the shared `history-row`).

use gpui::ElementId;
use storage::HistoryEntry;
use time::OffsetDateTime;

use super::{HistoryRow, entry_element_id, row_element_id, target_debug_selector};

/// One entry row carrying `id` — or carrying none.
fn entry_row(id: Option<i64>) -> HistoryRow {
    HistoryRow::Entry(Box::new(HistoryEntry {
        id,
        workspace_id: "workspace-under-test".to_owned(),
        created_at: OffsetDateTime::from_unix_timestamp(0)
            .unwrap_or_else(|error| panic!("the epoch is a valid timestamp: {error}")),
        protocol: "http".to_owned(),
        method: "GET".to_owned(),
        target: "https://api.test/users".to_owned(),
        status: Some(200),
        duration_ms: Some(142),
        request_json: serde_json::json!({}),
        response_summary_json: None,
        response_body_prefix: None,
    }))
}

#[test]
fn rows_the_store_left_without_an_id_never_share_their_element_id() {
    let first = entry_row(None);
    let second = entry_row(None);
    assert_ne!(
        row_element_id(Some(&first), 0),
        row_element_id(Some(&second), 1),
        "two rows without a stored id draw under their own element ids"
    );
    assert_ne!(
        entry_element_id(None, 0),
        entry_element_id(None, 1),
        "their row containers differ too"
    );
    assert_ne!(
        target_debug_selector(None, 0),
        target_debug_selector(None, 1),
        "and so do their test selectors"
    );
    assert_ne!(
        row_element_id(None, 0),
        row_element_id(None, 1),
        "an index past the rows snapshot gets its own fallback"
    );
}

#[test]
fn a_fallback_element_id_never_impersonates_a_stored_one() {
    // The row at position 7 carries no id; a neighbour stores id 7.
    assert_ne!(
        entry_element_id(None, 7),
        entry_element_id(Some(7), 8),
        "a fallback must not collide with a stored id"
    );
}

#[test]
fn rows_with_a_stored_id_keep_the_stable_id_derived_from_it() {
    let row = entry_row(Some(7));
    assert_eq!(
        row_element_id(Some(&row), 3),
        ElementId::from("history-entry-7"),
        "a stored id survives search re-ordering, whatever the row's position"
    );
    assert_eq!(
        entry_element_id(Some(7), 3),
        ElementId::from("history-entry-7")
    );
    assert_eq!(target_debug_selector(Some(7), 3), "history-entry-7-target");
}
