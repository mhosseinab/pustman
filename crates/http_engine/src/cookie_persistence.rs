//! The cookie jar's persistence through `storage` (REQ-WSP-06): a workspace's jar
//! loads from and saves to the `cookies` table on a background task, and workspace
//! files never hold cookies. Cookies are local state, so SQLite holds them and no
//! workspace file ever does.
//!
//! `storage` is synchronous by design (ADR-0002), so every SQLite call runs inside the
//! spawned task and never on the foreground. Cookie values are secrets for logging
//! purposes (PRD §7): this module logs nothing, and every error string is fixed text —
//! a failed cookies read becomes one fixed message and a failed write another, whatever
//! error text `storage` produced (it can quote the row it failed on).
//!
//! The table is local, untrusted data. A row whose `attributes_json` parses as JSON but
//! not as a cookie is skipped and the rest of the jar still loads. A row that is not JSON
//! at all is different: `storage`'s `list` parses a workspace's rows in one read, so it
//! fails the whole load with the fixed read message — a known limitation, lifted only by
//! a per-row-tolerant read in `storage`.

use std::sync::{Arc, Mutex, MutexGuard};

use cookie_store::{Cookie as StoreCookie, CookieStore};
use gpui::{BackgroundExecutor, Task};
use storage::{Cookie as CookieRow, LocalStore, StorageError};

use crate::cookies::{CookieJar, PUBLIC_SUFFIX_LIST};

#[cfg(test)]
#[path = "cookie_persistence_tests.rs"]
mod tests;

/// Loads the workspace's cookie jar from `storage`'s `cookies` table on the background
/// executor. Rows whose attributes parse as JSON but not as a cookie are skipped, rows
/// whose domain is a public suffix are dropped (RFC 6265 §5.3 step 5), and expired
/// cookies are dropped — the table is local, untrusted data, so one broken row is never a
/// panic. A row that is not JSON at all is not skippable: `storage`'s `list` fails the
/// whole read on it, with the one fixed-text read error (a known limitation, lifted only
/// by a per-row-tolerant read in `storage`). The whole table read runs inside the task
/// (ADR-0002).
pub fn load_jar(
    executor: &BackgroundExecutor,
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
) -> Task<anyhow::Result<CookieJar>> {
    executor.spawn(async move { load_from_store(&store, &workspace_id) })
}

/// Saves the workspace's cookie jar to `storage`'s `cookies` table on the background
/// executor. Afterwards the table holds exactly the jar's unexpired cookies for
/// `workspace_id`: rows for cookies the jar dropped are deleted and the rest are
/// inserted (the table's `insert` is `INSERT OR REPLACE`). Other workspaces' rows are
/// never touched.
///
/// The jar is snapshotted into owned rows on the caller's thread (cheap, no IO) so the
/// task owns everything it needs; every SQLite call runs inside the task (ADR-0002).
/// `storage` has no transaction API, so a save is not atomic: a task interrupted
/// halfway can leave the table partially replaced until the next save.
pub fn save_jar(
    executor: &BackgroundExecutor,
    store: Arc<Mutex<LocalStore>>,
    workspace_id: String,
    jar: &CookieJar,
) -> Task<anyhow::Result<()>> {
    let rows = snapshot_rows(&workspace_id, jar);
    executor.spawn(async move {
        let rows = rows?;
        save_to_store(&store, &workspace_id, &rows)
    })
}

/// The jar's unexpired cookies as `storage` rows. Each row keeps cookie_store's own
/// serialized cookie in `attributes_json` — the host-only and domain flags a reload
/// must restore live there — with the columns the dialog and the primary key read.
/// Expired cookies are not written.
fn snapshot_rows(workspace_id: &str, jar: &CookieJar) -> anyhow::Result<Vec<CookieRow>> {
    let mut rows = Vec::new();
    for cookie in jar.cookies() {
        let attributes_json = serde_json::to_value(cookie)
            .map_err(|_| anyhow::anyhow!("a stored cookie could not be serialized"))?;
        rows.push(CookieRow {
            workspace_id: workspace_id.to_owned(),
            domain: String::from(&cookie.domain),
            path: String::from(&cookie.path),
            name: cookie.name().to_owned(),
            value: cookie.value().to_owned(),
            attributes_json,
        });
    }
    // cookie_store iterates its map in arbitrary order; the rows go to SQLite sorted so
    // the write order does not jitter between saves.
    rows.sort_by(|left, right| {
        (&left.domain, &left.path, &left.name).cmp(&(&right.domain, &right.path, &right.name))
    });
    Ok(rows)
}

/// The jar rebuilt from the workspace's rows. Reloaded through cookie_store's own
/// serde, not [`CookieJar::add`], which would narrow a `Domain=` cookie to a host-only
/// one. A row that parses as JSON but not as a cookie is skipped; a row that is not JSON
/// at all fails the whole read in `storage` (see the module docs).
fn load_from_store(store: &Mutex<LocalStore>, workspace_id: &str) -> anyhow::Result<CookieJar> {
    let rows = {
        let store_guard = lock_store(store)?;
        store_guard
            .cookies()
            .list(workspace_id)
            .map_err(read_failure)?
    };
    let cookies: Vec<StoreCookie<'static>> = rows
        .into_iter()
        // A row that parses as JSON but not into a cookie is skipped, and its error is
        // dropped unread: that error text can quote the row's value (PRD §7).
        .filter_map(|row| serde_json::from_value(row.attributes_json).ok())
        // RFC 6265 §5.3 step 5 again for rows a tampered table wrote past the jar:
        // `CookieStore::from_cookies` runs no public-suffix check, so a `Domain=com` row
        // would reload as a supercookie for every `*.com` host. The same vendored list a
        // fresh jar carries decides.
        .filter(|cookie: &StoreCookie<'static>| {
            !cookie.domain.is_public_suffix(&PUBLIC_SUFFIX_LIST)
        })
        .collect();
    // `include_expired = false` drops what expired since its row was written. The items
    // are all `Ok`, so `from_cookies` cannot fail here.
    let store = CookieStore::from_cookies(cookies.into_iter().map(Ok::<_, anyhow::Error>), false)?;
    Ok(CookieJar::from_store(store))
}

/// Replaces the workspace's rows with `rows`, touching no other workspace: rows whose
/// cookie is gone are deleted, the rest are inserted.
fn save_to_store(
    store: &Mutex<LocalStore>,
    workspace_id: &str,
    rows: &[CookieRow],
) -> anyhow::Result<()> {
    let store_guard = lock_store(store)?;
    let repository = store_guard.cookies();
    let kept: std::collections::HashSet<(&str, &str, &str)> = rows
        .iter()
        .map(|row| (row.domain.as_str(), row.path.as_str(), row.name.as_str()))
        .collect();
    for row in repository.list(workspace_id).map_err(write_failure)? {
        if !kept.contains(&(row.domain.as_str(), row.path.as_str(), row.name.as_str())) {
            repository
                .delete(workspace_id, &row.domain, &row.path, &row.name)
                .map_err(write_failure)?;
        }
    }
    for row in rows {
        repository.insert(row).map_err(write_failure)?;
    }
    Ok(())
}

/// The store handle: a poisoned lock is an error, never a panic. The lock is only ever
/// held inside the background task, across calls that cannot yield.
fn lock_store(store: &Mutex<LocalStore>) -> anyhow::Result<MutexGuard<'_, LocalStore>> {
    store
        .lock()
        .map_err(|_| anyhow::anyhow!("the local store was left mid-operation by a failed task"))
}

/// The one message a failed cookies read shows, whatever failed in `storage`. The
/// `StorageError` behind it is dropped unread: its text can quote the row it failed on
/// and a cookie value never reaches an error string (PRD §7).
fn read_failure(_error: StorageError) -> anyhow::Error {
    anyhow::anyhow!("The saved cookies could not be read")
}

/// The one message a failed cookies write shows; see [`read_failure`] for why the
/// `StorageError` behind it is dropped unread.
fn write_failure(_error: StorageError) -> anyhow::Error {
    anyhow::anyhow!("The saved cookies could not be written")
}
