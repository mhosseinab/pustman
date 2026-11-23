//! #101: a failed history prune reaches the user — the one fixed message through the
//! report the caller installs — instead of being only logged (AGENTS.md: errors reach
//! the user). The prune fails for real here: the store's schema is rolled back before
//! the write, so the prune's `DELETE FROM history` finds no table and errors — no fault
//! switch of our own. Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use storage::LocalStore;

use super::PRUNE_FAILURE_MESSAGE;
use crate::HistoryRecorder;

/// The workspace every recorded row is written for.
const WORKSPACE: &str = "workspace-under-test";

/// A non-zero retention window: `0` disables pruning and no prune could fail.
const RETENTION_DAYS: u32 = 30;

/// A failed prune is reported to the user as the one fixed message, through the report
/// the caller installed: `storage`'s error text can quote the rows it failed on, and
/// rows are request content (PRD §7) — so the report carries wording, never an error.
#[test]
fn failed_prune_is_reported_to_the_user() {
    let store = rolled_back_store();
    let reported: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = HistoryRecorder::new(Arc::clone(&store), WORKSPACE)
        .with_retention_days(RETENTION_DAYS)
        .with_prune_failure_report({
            let reported = Arc::clone(&reported);
            move |message| {
                reported.lock().expect("the lock is free").push(message);
            }
        });

    // One recorded write drives the prune, as in production.
    let draft = recorder.draft(&plain_request()).expect("the draft builds");
    draft.record_cancelled();

    let messages = reported.lock().expect("the lock is free");
    assert_eq!(
        messages.len(),
        1,
        "the failed prune is reported exactly once: the gate claims before the delete"
    );
    assert_eq!(
        messages.first().copied(),
        Some(PRUNE_FAILURE_MESSAGE),
        "the failure reaches the user as the one fixed message"
    );
}

/// A store whose schema is rolled back: the retention failure to report — every write
/// against it errors, and `record_cancelled` still reaches the prune behind the insert.
fn rolled_back_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    store.migrate_down_to(0).expect("the schema rolls back");
    Arc::new(Mutex::new(store))
}

/// One GET to `https://example.test/users` with no params, no body and no own auth: the
/// smallest request [`HistoryRecorder::draft`] accepts.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "retention report test".to_string(),
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
