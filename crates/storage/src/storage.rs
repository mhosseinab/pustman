//! `storage` — Pustman's per-user local store (PRD §6.1 REQ-WSP-06, §8.5; ADR-0002).
//!
//! One SQLite database holds the local state that must never land in workspace files:
//! history, open tabs, layout, local variable values, cookies, recent workspaces and
//! small key-value flags. The schema is created and rolled back by versioned SQL pairs
//! under `migrations/`, embedded with `include_str!` and tracked in the `schema_migrations`
//! table.
//!
//! **No secrets are ever stored here.** Tokens, passwords and secret variable values live
//! in the OS keychain (`secrets` crate, ADR-0002); rows may only carry a reference to
//! them. History rows are redacted by the caller before insertion (PRD REQ-HIST-03).
//!
//! The API is synchronous by design (ADR-0002): callers run it off the foreground thread
//! with `cx.background_spawn`, and this crate spawns no threads itself.

use rusqlite::{Connection, Row, params, types::Type};
use std::path::Path;
use time::{Date, OffsetDateTime, SignedDuration, Time};

/// Everything that can go wrong while opening, migrating or querying the local store.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// SQLite could not open the database or rejected a statement.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// A `*_json` column did not hold a JSON document.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored timestamp cannot be represented as a [`OffsetDateTime`].
    #[error("time: {0}")]
    Time(#[from] time::error::ComponentRange),
    /// The database records a migration this build does not know: it was created by a
    /// newer Pustman, or a divergent build renumbered or removed a migration.
    #[error("database schema migration {version} is unknown to this build")]
    UnknownMigration { version: i64 },
    /// A migration below the database's highest applied version was never applied: a
    /// divergent build, or a `schema_migrations` edited by hand, left the schema in a
    /// state this build's migrations do not produce.
    #[error("database schema migration {version} was never applied, but a later one was")]
    MissingMigration { version: i64 },
    /// [`LocalStore::migrate_down_to`] was asked for a negative target version.
    #[error("invalid migration target version {0}")]
    InvalidTargetVersion(i64),
}

/// Convenience alias for the fallible operations in this crate.
pub type Result<T> = std::result::Result<T, StorageError>;

/// One reversible schema change: SQL that applies it and SQL that undoes it.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// Schema version this migration produces; its key in `schema_migrations`.
    pub version: i64,
    /// Short name, matching the `NNNN_name` file stem in `migrations/`.
    pub name: &'static str,
    /// SQL that applies the migration.
    pub up_sql: &'static str,
    /// SQL that reverses the migration. Every migration ships a working down, proven by
    /// `every_migration_has_working_down`.
    pub down_sql: &'static str,
}

/// Every migration this build knows, in ascending `version` order. Each entry is one
/// `NNNN_name.up.sql` / `NNNN_name.down.sql` pair in `migrations/`.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    up_sql: include_str!("../migrations/0001_init.up.sql"),
    down_sql: include_str!("../migrations/0001_init.down.sql"),
}];

/// The per-user local store: a SQLite database plus its migration state.
///
/// **No secrets are ever stored here** — secret values live in the OS keychain
/// (`secrets` crate, ADR-0002). Call [`LocalStore::migrate`] once after opening.
pub struct LocalStore {
    connection: Connection,
}

impl LocalStore {
    /// Opens the database at `path`, creating the file if needed. The schema is left
    /// untouched; call [`LocalStore::migrate`] to bring it up to date.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            connection: Connection::open(path)?,
        })
    }

    /// Opens a private in-memory database, mostly for tests.
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            connection: Connection::open_in_memory()?,
        })
    }

    /// Applies every migration this build knows about that the database has not seen
    /// yet, each in its own transaction, recording it in `schema_migrations`. Running it
    /// twice is a no-op.
    ///
    /// Fails with [`StorageError::UnknownMigration`] when the database records a version
    /// this build has no SQL for (a newer Pustman migrated it, or a divergent build
    /// renumbered a migration), and with [`StorageError::MissingMigration`] when a
    /// migration below the highest applied one was never applied — rather than layering
    /// new DDL over a schema that is not the one these migrations produce.
    pub fn migrate(&mut self) -> Result<()> {
        ensure_schema_migrations(&self.connection)?;
        if let Some(error) = ledger_inconsistency(&self.connection, MIGRATIONS)? {
            return Err(error);
        }
        for migration in MIGRATIONS {
            if is_applied(&self.connection, migration.version)? {
                continue;
            }
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(migration.up_sql)?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![migration.version, migration.name, now_millis()],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Rolls back every applied migration above `version`, newest first, running each
    /// migration's `down` in its own transaction. [`LocalStore::migrate`] after it
    /// re-applies the rolled-back migrations.
    ///
    /// `migrate_down_to(0)` leaves no local-state table behind; the `schema_migrations`
    /// bookkeeping table itself is kept and simply holds no rows. Fails with
    /// [`StorageError::UnknownMigration`] or [`StorageError::MissingMigration`] when the
    /// recorded migrations are not exactly the ones this build knows.
    pub fn migrate_down_to(&mut self, version: i64) -> Result<()> {
        if version < 0 {
            return Err(StorageError::InvalidTargetVersion(version));
        }
        ensure_schema_migrations(&self.connection)?;
        if let Some(error) = ledger_inconsistency(&self.connection, MIGRATIONS)? {
            return Err(error);
        }
        let mut rollback = Vec::new();
        for migration in MIGRATIONS.iter().rev() {
            if migration.version > version && is_applied(&self.connection, migration.version)? {
                rollback.push(*migration);
            }
        }
        for migration in rollback {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(migration.down_sql)?;
            transaction.execute(
                "DELETE FROM schema_migrations WHERE version = ?1",
                params![migration.version],
            )?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// The highest applied migration version, or 0 when nothing has been applied yet.
    pub fn schema_version(&self) -> Result<i64> {
        if !table_exists(&self.connection, "schema_migrations")? {
            return Ok(0);
        }
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    /// Key-value flags: dismissed banners, the last version run, and other small
    /// non-secret settings.
    pub fn kv(&self) -> KvRepo<'_> {
        KvRepo {
            connection: &self.connection,
        }
    }

    /// Recorded request history (PRD §8.5).
    pub fn history(&self) -> HistoryRepo<'_> {
        HistoryRepo {
            connection: &self.connection,
        }
    }

    /// Open tabs per workspace, in tab order.
    pub fn open_tabs(&self) -> OpenTabsRepo<'_> {
        OpenTabsRepo {
            connection: &self.connection,
        }
    }

    /// Replaces one workspace's `open_tabs` set with `tabs`, in one transaction: the rows
    /// that are gone and the rows that arrive land together, so a save interrupted
    /// halfway leaves the previous set in place instead of half of each (PRD REQ-UI-02).
    ///
    /// It lives here rather than on [`OpenTabsRepo`] because the transaction needs the
    /// connection this type owns. Every row is stored under `workspace_id` whatever its
    /// [`OpenTab::workspace_id`] says, and a `tabs` list holding the same item twice is a
    /// caller bug the whole call fails on: the transaction rolls back and nothing is
    /// written.
    pub fn replace_workspace_tabs(&mut self, workspace_id: &str, tabs: &[OpenTab]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM open_tabs WHERE workspace_id = ?1",
            params![workspace_id],
        )?;
        for tab in tabs {
            transaction.execute(
                "INSERT INTO open_tabs (workspace_id, position, item_id, pinned, draft_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    workspace_id,
                    tab.position,
                    tab.item_id,
                    tab.pinned,
                    draft_json_column(tab.draft_json.as_ref())?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Persisted UI layout per workspace.
    pub fn layout(&self) -> LayoutRepo<'_> {
        LayoutRepo {
            connection: &self.connection,
        }
    }

    /// Non-secret variable values kept locally per scope.
    pub fn local_values(&self) -> LocalValuesRepo<'_> {
        LocalValuesRepo {
            connection: &self.connection,
        }
    }

    /// The per-workspace cookie jar.
    pub fn cookies(&self) -> CookiesRepo<'_> {
        CookiesRepo {
            connection: &self.connection,
        }
    }

    /// Workspaces recently opened on this machine.
    pub fn recent_workspaces(&self) -> RecentWorkspacesRepo<'_> {
        RecentWorkspacesRepo {
            connection: &self.connection,
        }
    }
}

/// One row of `kv`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

/// One row of `history` (PRD §8.5).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    /// Assigned by [`HistoryRepo::insert`]; `None` before the entry is stored.
    pub id: Option<i64>,
    pub workspace_id: String,
    pub created_at: OffsetDateTime,
    pub protocol: String,
    pub method: String,
    pub target: String,
    pub status: Option<i64>,
    pub duration_ms: Option<i64>,
    /// The request as it was before variable resolution, as a JSON document
    /// (PRD REQ-HIST-02).
    pub request_json: serde_json::Value,
    /// Stored response summary, as a JSON document.
    pub response_summary_json: Option<serde_json::Value>,
    /// First bytes of the response body (PRD REQ-HIST-02), already redacted by the
    /// caller (PRD REQ-HIST-03).
    pub response_body_prefix: Option<Vec<u8>>,
}

/// One row of `open_tabs`.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTab {
    pub workspace_id: String,
    /// 0-based tab order within the workspace.
    pub position: i64,
    pub item_id: String,
    pub pinned: bool,
    /// The unsaved tab draft as a JSON document, when the tab has one (PRD REQ-NFR-05).
    pub draft_json: Option<serde_json::Value>,
}

/// One row of `layout`: the workspace's persisted UI layout as a JSON document.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub workspace_id: String,
    pub json: serde_json::Value,
}

/// One row of `local_values`: a variable value kept locally for one scope. Non-secret
/// values only — secrets live in the keychain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalValue {
    pub workspace_id: String,
    pub scope_id: String,
    pub key: String,
    pub value: String,
}

/// One row of `cookies`: one cookie of a workspace's jar.
#[derive(Clone, Debug, PartialEq)]
pub struct Cookie {
    pub workspace_id: String,
    pub domain: String,
    pub path: String,
    pub name: String,
    pub value: String,
    /// Cookie attributes (expiry, flags, …) as a JSON document.
    pub attributes_json: serde_json::Value,
}

/// One row of `recent_workspaces`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentWorkspace {
    pub path: String,
    pub opened_at: OffsetDateTime,
}

/// Access to the `kv` table.
pub struct KvRepo<'a> {
    connection: &'a Connection,
}

impl KvRepo<'_> {
    /// Inserts or replaces one flag.
    pub fn insert(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Every flag, ordered by key.
    pub fn list(&self) -> Result<Vec<KeyValue>> {
        let mut statement = self
            .connection
            .prepare("SELECT key, value FROM kv ORDER BY key")?;
        let rows = statement.query_map([], |row| {
            Ok(KeyValue {
                key: row.get("key")?,
                value: row.get("value")?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes one flag.
    pub fn delete(&self, key: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM kv WHERE key = ?1", params![key])?;
        Ok(())
    }
}

/// Access to the `history` table (PRD §8.5).
pub struct HistoryRepo<'a> {
    connection: &'a Connection,
}

impl HistoryRepo<'_> {
    /// Stores one entry and returns the assigned [`HistoryEntry::id`].
    pub fn insert(&self, entry: &HistoryEntry) -> Result<i64> {
        let request_json = serde_json::to_string(&entry.request_json)?;
        let response_summary_json = entry
            .response_summary_json
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.connection.execute(
            "INSERT INTO history (workspace_id, created_at, protocol, method, target, status,
                                  duration_ms, request_json, response_summary_json,
                                  response_body_prefix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.workspace_id,
                to_millis(entry.created_at),
                entry.protocol,
                entry.method,
                entry.target,
                entry.status,
                entry.duration_ms,
                request_json,
                response_summary_json,
                entry.response_body_prefix,
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Entries recorded on `day` (UTC), newest first, ties broken by `id` so two sends
    /// in the same millisecond still come back in a total order.
    pub fn list_by_day(&self, workspace_id: &str, day: Date) -> Result<Vec<HistoryEntry>> {
        let (day_start, day_end) = day_bounds(day);
        let sql = format!(
            "SELECT {HISTORY_COLUMNS} FROM history
             WHERE workspace_id = ?1 AND created_at >= ?2 AND created_at < ?3
             ORDER BY created_at DESC, id DESC"
        );
        self.query_history(&sql, params![workspace_id, day_start, day_end])
    }

    /// Entries whose target, method or protocol contains `query` (ASCII
    /// case-insensitive), newest first with the same `id` tie-break as
    /// [`HistoryRepo::list_by_day`]. History is per workspace, so the search is scoped
    /// to `workspace_id`.
    pub fn search(&self, workspace_id: &str, query: &str) -> Result<Vec<HistoryEntry>> {
        let pattern = format!("%{}%", escape_like(query));
        let sql = format!(
            "SELECT {HISTORY_COLUMNS} FROM history
             WHERE workspace_id = ?1 AND ({LIKE_QUERY})
             ORDER BY created_at DESC, id DESC"
        );
        self.query_history(&sql, params![workspace_id, pattern])
    }

    /// Deletes one entry of one workspace by id. Scoped by `workspace_id` like every
    /// other delete in this crate, so a caller threading an id across workspaces cannot
    /// remove another workspace's row.
    pub fn delete(&self, workspace_id: &str, id: i64) -> Result<()> {
        self.connection.execute(
            "DELETE FROM history WHERE workspace_id = ?1 AND id = ?2",
            params![workspace_id, id],
        )?;
        Ok(())
    }

    /// Deletes every entry recorded for one workspace (PRD REQ-HIST-04 "clear all").
    pub fn clear(&self, workspace_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM history WHERE workspace_id = ?1",
            params![workspace_id],
        )?;
        Ok(())
    }

    /// Deletes every entry older than `days` days, across workspaces — the retention
    /// setting is global (PRD REQ-HIST-04) — and returns how many rows were removed.
    ///
    /// `days = 0` disables retention and keeps every entry; it never wipes history. "0
    /// disables" is the encoding this settings family uses, not something REQ-HIST-04
    /// defines: PRD REQ-HIST-02 is the precedent, using the same encoding for the
    /// body-storage limit. A window too long for `OffsetDateTime` to represent keeps
    /// everything as well, rather than panicking.
    pub fn prune_older_than(&self, days: u32) -> Result<usize> {
        if days == 0 {
            return Ok(0);
        }
        // `checked_sub`, not `-`: `time`'s `Sub` panics outside the representable range
        // and `days` is caller data (a user setting). An unrepresentable cutoff means the
        // retention window predates every possible record, so nothing is older than it.
        let cutoff = OffsetDateTime::now_utc()
            .checked_sub(SignedDuration::days(i64::from(days)))
            .map_or(i64::MIN, to_millis);
        Ok(self
            .connection
            .execute("DELETE FROM history WHERE created_at < ?1", params![cutoff])?)
    }

    fn query_history(
        &self,
        sql: &str,
        arguments: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<HistoryEntry>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(arguments, history_entry_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

/// Access to the `open_tabs` table.
pub struct OpenTabsRepo<'a> {
    connection: &'a Connection,
}

impl OpenTabsRepo<'_> {
    /// Inserts or replaces the tab with the same workspace and item.
    pub fn insert(&self, tab: &OpenTab) -> Result<()> {
        let draft_json = draft_json_column(tab.draft_json.as_ref())?;
        self.connection.execute(
            "INSERT OR REPLACE INTO open_tabs (workspace_id, position, item_id, pinned, draft_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                tab.workspace_id,
                tab.position,
                tab.item_id,
                tab.pinned,
                draft_json
            ],
        )?;
        Ok(())
    }

    /// The workspace's tabs in tab order (ascending `position`, ties broken by
    /// `item_id` so the order stays total when callers reuse a position).
    pub fn list(&self, workspace_id: &str) -> Result<Vec<OpenTab>> {
        let mut statement = self.connection.prepare(
            "SELECT workspace_id, position, item_id, pinned, draft_json FROM open_tabs
             WHERE workspace_id = ?1 ORDER BY position, item_id",
        )?;
        let rows = statement.query_map(params![workspace_id], |row| {
            let draft_json: Option<String> = row.get("draft_json")?;
            let draft_json = draft_json
                .map(|json| serde_json::from_str(&json))
                .transpose()
                .map_err(|error| conversion_failure(4, Type::Text, error))?;
            Ok(OpenTab {
                workspace_id: row.get("workspace_id")?,
                position: row.get("position")?,
                item_id: row.get("item_id")?,
                pinned: row.get("pinned")?,
                draft_json,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes one tab by item.
    pub fn delete(&self, workspace_id: &str, item_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM open_tabs WHERE workspace_id = ?1 AND item_id = ?2",
            params![workspace_id, item_id],
        )?;
        Ok(())
    }
}

/// Access to the `layout` table.
pub struct LayoutRepo<'a> {
    connection: &'a Connection,
}

impl LayoutRepo<'_> {
    /// Inserts or replaces one workspace's layout.
    pub fn insert(&self, layout: &Layout) -> Result<()> {
        let json = serde_json::to_string(&layout.json)?;
        self.connection.execute(
            "INSERT OR REPLACE INTO layout (workspace_id, json) VALUES (?1, ?2)",
            params![layout.workspace_id, json],
        )?;
        Ok(())
    }

    /// Every stored layout, ordered by workspace.
    pub fn list(&self) -> Result<Vec<Layout>> {
        let mut statement = self
            .connection
            .prepare("SELECT workspace_id, json FROM layout ORDER BY workspace_id")?;
        let rows = statement.query_map([], |row| {
            let json: String = row.get("json")?;
            Ok(Layout {
                workspace_id: row.get("workspace_id")?,
                json: serde_json::from_str(&json)
                    .map_err(|error| conversion_failure(1, Type::Text, error))?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes one workspace's layout.
    pub fn delete(&self, workspace_id: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM layout WHERE workspace_id = ?1",
            params![workspace_id],
        )?;
        Ok(())
    }
}

/// Access to the `local_values` table.
pub struct LocalValuesRepo<'a> {
    connection: &'a Connection,
}

impl LocalValuesRepo<'_> {
    /// Inserts or replaces one value.
    pub fn insert(&self, local_value: &LocalValue) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO local_values (workspace_id, scope_id, key, value)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                local_value.workspace_id,
                local_value.scope_id,
                local_value.key,
                local_value.value
            ],
        )?;
        Ok(())
    }

    /// Every value stored for one workspace, ordered by scope then key.
    pub fn list(&self, workspace_id: &str) -> Result<Vec<LocalValue>> {
        let mut statement = self.connection.prepare(
            "SELECT workspace_id, scope_id, key, value FROM local_values
             WHERE workspace_id = ?1 ORDER BY scope_id, key",
        )?;
        let rows = statement.query_map(params![workspace_id], |row| {
            Ok(LocalValue {
                workspace_id: row.get("workspace_id")?,
                scope_id: row.get("scope_id")?,
                key: row.get("key")?,
                value: row.get("value")?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes one value.
    pub fn delete(&self, workspace_id: &str, scope_id: &str, key: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM local_values WHERE workspace_id = ?1 AND scope_id = ?2 AND key = ?3",
            params![workspace_id, scope_id, key],
        )?;
        Ok(())
    }
}

/// Access to the `cookies` table.
pub struct CookiesRepo<'a> {
    connection: &'a Connection,
}

impl CookiesRepo<'_> {
    /// Inserts or replaces one cookie.
    pub fn insert(&self, cookie: &Cookie) -> Result<()> {
        let attributes_json = serde_json::to_string(&cookie.attributes_json)?;
        self.connection.execute(
            "INSERT OR REPLACE INTO cookies (workspace_id, domain, path, name, value, attributes_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                cookie.workspace_id,
                cookie.domain,
                cookie.path,
                cookie.name,
                cookie.value,
                attributes_json
            ],
        )?;
        Ok(())
    }

    /// Every cookie of one workspace, ordered by domain, path and name.
    pub fn list(&self, workspace_id: &str) -> Result<Vec<Cookie>> {
        let mut statement = self.connection.prepare(
            "SELECT workspace_id, domain, path, name, value, attributes_json FROM cookies
             WHERE workspace_id = ?1 ORDER BY domain, path, name",
        )?;
        let rows = statement.query_map(params![workspace_id], |row| {
            let attributes_json: String = row.get("attributes_json")?;
            Ok(Cookie {
                workspace_id: row.get("workspace_id")?,
                domain: row.get("domain")?,
                path: row.get("path")?,
                name: row.get("name")?,
                value: row.get("value")?,
                attributes_json: serde_json::from_str(&attributes_json)
                    .map_err(|error| conversion_failure(5, Type::Text, error))?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes one cookie.
    pub fn delete(&self, workspace_id: &str, domain: &str, path: &str, name: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM cookies WHERE workspace_id = ?1 AND domain = ?2 AND path = ?3 AND name = ?4",
            params![workspace_id, domain, path, name],
        )?;
        Ok(())
    }
}

/// Access to the `recent_workspaces` table.
pub struct RecentWorkspacesRepo<'a> {
    connection: &'a Connection,
}

impl RecentWorkspacesRepo<'_> {
    /// Records a workspace as opened now-in `opened_at` — replacing any earlier entry
    /// for the same path.
    pub fn insert(&self, recent: &RecentWorkspace) -> Result<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO recent_workspaces (path, opened_at) VALUES (?1, ?2)",
            params![recent.path, to_millis(recent.opened_at)],
        )?;
        Ok(())
    }

    /// Every recorded workspace, most recently opened first.
    pub fn list(&self) -> Result<Vec<RecentWorkspace>> {
        let mut statement = self.connection.prepare(
            "SELECT path, opened_at FROM recent_workspaces ORDER BY opened_at DESC, path",
        )?;
        let rows = statement.query_map([], |row| {
            let opened_at: i64 = row.get("opened_at")?;
            Ok(RecentWorkspace {
                path: row.get("path")?,
                opened_at: from_millis(opened_at)
                    .map_err(|error| conversion_failure(1, Type::Integer, error))?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Forgets one workspace.
    pub fn delete(&self, path: &str) -> Result<()> {
        self.connection.execute(
            "DELETE FROM recent_workspaces WHERE path = ?1",
            params![path],
        )?;
        Ok(())
    }
}

const HISTORY_COLUMNS: &str = "id, workspace_id, created_at, protocol, method, target, status, \
                               duration_ms, request_json, response_summary_json, \
                               response_body_prefix";

const LIKE_QUERY: &str = "target LIKE ?2 ESCAPE '\\' OR method LIKE ?2 ESCAPE '\\' OR \
                          protocol LIKE ?2 ESCAPE '\\'";

const MILLIS_PER_DAY: i64 = 86_400_000;

const NANOS_PER_MILLI: i128 = 1_000_000;

/// The `draft_json` column form of a tab draft (PRD REQ-NFR-05): the item's opaque text
/// as a JSON string document, when the tab has one.
fn draft_json_column(draft_json: Option<&serde_json::Value>) -> Result<Option<String>> {
    Ok(draft_json.map(serde_json::to_string).transpose()?)
}

/// The stored form of a timestamp: unix milliseconds, UTC.
fn to_millis(timestamp: OffsetDateTime) -> i64 {
    timestamp.unix_timestamp_nanos().div_euclid(NANOS_PER_MILLI) as i64
}

/// Reads stored unix milliseconds back into a timestamp.
fn from_millis(millis: i64) -> std::result::Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * NANOS_PER_MILLI)
}

fn ensure_schema_migrations(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER NOT NULL PRIMARY KEY,
            name       TEXT    NOT NULL,
            applied_at INTEGER NOT NULL
        ) STRICT;",
    )?;
    Ok(())
}

/// The migration state `known` cannot reconcile, if any.
///
/// Membership in `known`, not a version range: a migration a divergent build renumbered
/// or removed leaves an *interior* ledger row that `migrate` must not layer new DDL over,
/// and a missing interior row is the same problem from the other side. `known` is a
/// parameter so the real query path can be tested against a divergent ledger.
fn ledger_inconsistency(
    connection: &Connection,
    known: &[Migration],
) -> Result<Option<StorageError>> {
    let mut statement =
        connection.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let applied = statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    if let Some(version) = first_unknown_version(&applied, known) {
        return Ok(Some(StorageError::UnknownMigration { version }));
    }
    Ok(
        first_gap_version(&applied, known)
            .map(|version| StorageError::MissingMigration { version }),
    )
}

/// The lowest applied version with no entry in `known`, if any.
fn first_unknown_version(applied: &[i64], known: &[Migration]) -> Option<i64> {
    applied
        .iter()
        .copied()
        .find(|version| !known.iter().any(|migration| migration.version == *version))
}

/// The lowest version `known` has SQL for that was never applied even though a later one
/// was, if any. Such a gap means the schema is not the one these migrations produce:
/// re-running the missing migration would layer its DDL over a schema that already has it.
fn first_gap_version(applied: &[i64], known: &[Migration]) -> Option<i64> {
    let highest = applied.iter().copied().max()?;
    known
        .iter()
        .map(|migration| migration.version)
        .filter(|version| *version < highest && !applied.contains(version))
        .min()
}

fn is_applied(connection: &Connection, version: i64) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
        params![version],
        |row| row.get(0),
    )?)
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        params![name],
        |row| row.get(0),
    )?)
}

fn now_millis() -> i64 {
    to_millis(OffsetDateTime::now_utc())
}

fn day_bounds(day: Date) -> (i64, i64) {
    let day_start = to_millis(day.with_time(Time::MIDNIGHT).assume_utc());
    (day_start, day_start + MILLIS_PER_DAY)
}

/// Escapes `LIKE` wildcards so a search term matches literally.
fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// Wraps a conversion failure in the rusqlite error the row mapper must return.
fn conversion_failure(
    index: usize,
    kind: Type,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, kind, Box::new(error))
}

fn history_entry_from_row(row: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let created_at: i64 = row.get("created_at")?;
    let request_json: String = row.get("request_json")?;
    let response_summary_json: Option<String> = row.get("response_summary_json")?;
    Ok(HistoryEntry {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        created_at: from_millis(created_at)
            .map_err(|error| conversion_failure(2, Type::Integer, error))?,
        protocol: row.get("protocol")?,
        method: row.get("method")?,
        target: row.get("target")?,
        status: row.get("status")?,
        duration_ms: row.get("duration_ms")?,
        request_json: serde_json::from_str(&request_json)
            .map_err(|error| conversion_failure(8, Type::Text, error))?,
        response_summary_json: response_summary_json
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|error| conversion_failure(9, Type::Text, error))?,
        response_body_prefix: row.get("response_body_prefix")?,
    })
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "schema_objects_tests.rs"]
mod schema_objects_tests;
