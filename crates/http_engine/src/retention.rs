//! History retention (PRD REQ-HIST-04): rows older than the retention window are
//! pruned at most once per app session, from the background task that already writes
//! the row (ADR-0002), through `storage`'s own `prune_older_than` — this crate keeps no
//! retention SQL and no date math of its own.
//!
//! The window arrives as a value: `http_engine` is a domain crate and never reads
//! `settings`; the caller passes its `history_retention_days` (default 30). A failed
//! prune is one fixed message — `storage`'s error text can quote the rows it failed on,
//! and rows are request content (PRD §7) — and it never blocks or undoes the row the
//! write inserted.

use std::sync::atomic::{AtomicBool, Ordering};

use storage::LocalStore;

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

/// The retention window a [`crate::HistoryRecorder`] prunes by when its caller passes
/// none: REQ-HIST-04's default, the same 30 `settings` uses for
/// `history_retention_days`.
pub(crate) const DEFAULT_HISTORY_RETENTION_DAYS: u32 = 30;

/// The once-per-app-session claim behind [`prune_once`]. Every clone of a recorder —
/// and every draft holds one — shares one gate through an `Arc`, so concurrent writers
/// prune exactly once.
pub(crate) struct RetentionGate {
    claimed: AtomicBool,
}

impl RetentionGate {
    /// A fresh gate, created with the recorder: one per app session.
    pub(crate) fn new() -> Self {
        Self {
            claimed: AtomicBool::new(false),
        }
    }
}

/// Prune `store`'s history rows older than `retention_days` days (REQ-HIST-04), at most
/// once per app session. The claim is taken before the delete, so a prune is attempted
/// once even when it fails — the writer's one-row gate follows the same rule. `0`
/// disables pruning, as in `storage`'s `prune_older_than`. Runs inside the writer's
/// store-lock scope and holds no lock across an await (ADR-0002).
pub(crate) fn prune_once(gate: &RetentionGate, store: &LocalStore, retention_days: u32) {
    if gate.claimed.swap(true, Ordering::AcqRel) {
        return;
    }
    if store.history().prune_older_than(retention_days).is_err() {
        log::error!("history retention could not prune old entries");
    }
}
