//! History retention (PRD REQ-HIST-04): the writer prunes rows older than the
//! retention setting at most once per app session — the first write deletes, a later
//! one never does. Rows carry explicit timestamps, so no test waits on the clock, and
//! the writer path is driven directly: `storage` is synchronous (ADR-0002) and the
//! prune is part of the same write. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use storage::{HistoryEntry, LocalStore};
use time::{Duration, OffsetDateTime};

use crate::HistoryRecorder;

/// The retention window the test configures: older rows go, newer rows stay.
const RETENTION_DAYS: u32 = 30;

/// The workspace every recorded row is written for.
const WORKSPACE: &str = "workspace-under-test";

/// Rows older than the retention setting are pruned at most once per app session
/// (REQ-HIST-04): the first write after they age out deletes them, while rows written
/// after that first prune — however old — survive every later write, because the gate
/// is shared by every clone of the recorder.
#[test]
fn retention_prunes_entries_older_than_setting() {
    let store = open_store();
    insert_row(&store, "older-than-retention", days_ago(31));
    insert_row(&store, "within-retention", days_ago(1));
    let recorder =
        HistoryRecorder::new(store.clone(), WORKSPACE).with_retention_days(RETENTION_DAYS);

    // The first write triggers the prune (in production on the writer's background task).
    let first_draft = recorder.draft(&plain_request()).expect("the draft builds");
    first_draft.record_cancelled();

    let targets = stored_targets(&store);
    assert!(
        !targets
            .iter()
            .any(|target| target == "older-than-retention"),
        "the row older than the setting is pruned"
    );
    assert!(
        targets.iter().any(|target| target == "within-retention"),
        "the row within the setting stays"
    );

    // At most once per app session: this row is old enough to be pruned, but the first
    // prune already claimed the gate, so a second write (from a clone of the recorder)
    // deletes nothing.
    insert_row(&store, "older-after-first-prune", days_ago(31));
    let second_draft = recorder
        .clone()
        .draft(&plain_request())
        .expect("the draft builds");
    second_draft.record_cancelled();

    let targets = stored_targets(&store);
    assert!(
        targets
            .iter()
            .any(|target| target == "older-after-first-prune"),
        "a second write prunes nothing: the prune ran at most once"
    );
}

/// The in-memory local store the recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// One history row recorded `created_at`, marked by its `target` — written directly
/// through `storage`, as the step asks, so the timestamps are exactly what they read.
fn insert_row(store: &Mutex<LocalStore>, target: &str, created_at: OffsetDateTime) {
    let entry = HistoryEntry {
        id: None,
        workspace_id: WORKSPACE.to_string(),
        created_at,
        protocol: "http".to_string(),
        method: "GET".to_string(),
        target: target.to_string(),
        status: Some(200),
        duration_ms: Some(1),
        request_json: serde_json::json!({}),
        response_summary_json: None,
        response_body_prefix: None,
    };
    store
        .lock()
        .expect("the lock is free")
        .history()
        .insert(&entry)
        .expect("the row inserts");
}

/// What the `history` table holds, as the targets the markers were written into.
fn stored_targets(store: &Mutex<LocalStore>) -> Vec<String> {
    store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list")
        .into_iter()
        .map(|row| row.target)
        .collect()
}

/// `days` days before now, well inside `time`'s representable range.
fn days_ago(days: i64) -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::days(days)
}

/// One GET to `https://example.test/users` with no params, no body and no own auth: the
/// smallest request [`HistoryRecorder::draft`] accepts.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "retention test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}
