//! History retention (PRD REQ-HIST-04): rows older than the retention window are
//! pruned at most once per app session, from the background task that already writes
//! the row (ADR-0002), through `storage`'s own `prune_older_than` — this crate keeps no
//! retention SQL and no date math of its own.
//!
//! The window arrives as a value: `http_engine` is a domain crate and never reads
//! `settings`; the caller passes its `history_retention_days` (default 30) and can
//! update it in place through [`RetentionWindow`], so a settings change reaches an
//! installed recorder without a rebuild (issue #196). A failed prune is one fixed message — `storage`'s error text can quote the rows it failed on,
//! and rows are request content (PRD §7) — handed back to the writer, which reports it
//! to the user (AGENTS.md: errors reach the user); the prune never blocks or undoes the
//! row the write inserted.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use storage::LocalStore;

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retention_report_tests.rs"]
mod report_tests;

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

/// The one message a failed prune reports: `storage`'s error text can quote the rows it
/// failed on, and rows are request content (PRD §7) — the failure is folded into this
/// fixed wording and the error is dropped unread (the shape `history_panel`'s delete
/// failures take).
pub(crate) const PRUNE_FAILURE_MESSAGE: &str = "Old request history entries could not be removed";

/// The retention window (REQ-HIST-04) a [`crate::HistoryRecorder`] prunes by. Shared by
/// every clone of the recorder — the drafts of sends already in flight hold clones — so
/// a caller setting it from settings reaches all of them at once (issue #196) instead of
/// the value being frozen when the recorder was built.
#[derive(Clone)]
pub(crate) struct RetentionWindow(Arc<AtomicU32>);

impl RetentionWindow {
    /// A window of `retention_days` days.
    pub(crate) fn new(retention_days: u32) -> Self {
        Self(Arc::new(AtomicU32::new(retention_days)))
    }

    /// The window the writer prunes by right now.
    pub(crate) fn days(&self) -> u32 {
        self.0.load(Ordering::Acquire)
    }

    /// Set the window every clone prunes by from now on.
    pub(crate) fn set(&self, retention_days: u32) {
        self.0.store(retention_days, Ordering::Release);
    }
}

/// Prune `store`'s history rows older than `retention_days` days (REQ-HIST-04), at most
/// once per app session. The claim is taken before the delete, so a prune is attempted
/// once even when it fails — the writer's one-row gate follows the same rule. `0`
/// disables pruning, as in `storage`'s `prune_older_than`. Runs inside the writer's
/// store-lock scope and holds no lock across an await (ADR-0002).
///
/// Returns [`PRUNE_FAILURE_MESSAGE`] when the delete failed, for the writer to report
/// to the user (AGENTS.md: errors reach the user) — never the error itself.
pub(crate) fn prune_once(
    gate: &RetentionGate,
    store: &LocalStore,
    retention_days: u32,
) -> Option<&'static str> {
    if gate.claimed.swap(true, Ordering::AcqRel) {
        return None;
    }
    if store.history().prune_older_than(retention_days).is_err() {
        return Some(PRUNE_FAILURE_MESSAGE);
    }
    None
}
