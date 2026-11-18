use super::*;
use std::collections::BTreeSet;
use time::{OffsetDateTime, SignedDuration};

const TABLES: [&str; 7] = [
    "kv",
    "history",
    "open_tabs",
    "layout",
    "local_values",
    "cookies",
    "recent_workspaces",
];

fn latest_version() -> i64 {
    MIGRATIONS.last().map_or(0, |migration| migration.version)
}

/// Every schema object a database holds, as `type:name` plus the DDL that created it,
/// ignoring SQLite's own `sqlite_*` objects. Two databases with the same migrations
/// applied must agree on it. `type:name` alone cannot see a `down` that drops a *column*
/// of another migration's table: the table still exists, so the object set matches while
/// the column and its data are gone at the intermediate target. Folding the stored DDL
/// in makes the column set part of the comparison — a `down` must reverse its `up`
/// exactly, not merely leave the same objects behind.
fn schema_objects(connection: &Connection) -> Result<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let rows = statement.query_map([], |row| {
        let kind: String = row.get("type")?;
        let name: String = row.get("name")?;
        let ddl: String = row.get(2)?;
        Ok(format!("{kind}:{name}\n{ddl}"))
    })?;
    Ok(rows.collect::<rusqlite::Result<BTreeSet<_>>>()?)
}

/// The `(version, name)` a `migrations/NNNN_name{suffix}` file stands for.
fn migration_identity(file_name: &str, suffix: &str) -> (i64, String) {
    let stem = file_name
        .strip_suffix(suffix)
        .unwrap_or_else(|| panic!("{file_name} must end with {suffix}"));
    let (version, name) = stem
        .split_once('_')
        .unwrap_or_else(|| panic!("{file_name} must be named NNNN_name"));
    (
        version
            .parse()
            .unwrap_or_else(|error| panic!("{file_name} version: {error}")),
        name.to_string(),
    )
}

fn history_entry(workspace_id: &str, created_at: OffsetDateTime, target: &str) -> HistoryEntry {
    HistoryEntry {
        id: None,
        workspace_id: workspace_id.to_string(),
        created_at,
        protocol: "http".to_string(),
        method: "GET".to_string(),
        target: target.to_string(),
        status: Some(200),
        duration_ms: Some(12),
        request_json: serde_json::json!({ "method": "GET" }),
        response_summary_json: Some(serde_json::json!({ "status": 200 })),
        response_body_prefix: Some(b"{\"ok\":true}".to_vec()),
    }
}

#[test]
fn migrate_creates_all_tables() -> Result<()> {
    let directory = tempfile::tempdir().expect("temp directory");
    let mut store = LocalStore::open(directory.path().join("pustman.db"))?;
    store.migrate()?;
    for table in TABLES {
        assert!(
            table_exists(&store.connection, table)?,
            "table {table} must exist after migrate"
        );
    }
    assert!(table_exists(&store.connection, "schema_migrations")?);
    assert_eq!(store.schema_version()?, latest_version());
    Ok(())
}

#[test]
fn migrate_is_idempotent() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.migrate()?;
    assert_eq!(store.schema_version()?, latest_version());
    let ledger_rows: i64 =
        store
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
    assert_eq!(
        ledger_rows,
        MIGRATIONS.len() as i64,
        "a second migrate must not record migrations twice"
    );
    store.kv().insert("last.version", "1.0.0")?;
    store.migrate()?;
    assert_eq!(
        store.kv().list()?,
        vec![KeyValue {
            key: "last.version".to_string(),
            value: "1.0.0".to_string()
        }],
        "migrate must not touch local data"
    );
    Ok(())
}

#[test]
fn every_migration_has_working_down() -> Result<()> {
    for migration in MIGRATIONS {
        assert!(
            !migration.down_sql.trim().is_empty(),
            "migration {} must ship a down",
            migration.version
        );
    }
    // Rolling back to every reachable target version and migrating up again runs each
    // down in turn and proves it can be re-applied: every migration is reversible. The
    // rebuilt database is diffed against a freshly migrated one, because `schema_version`
    // alone cannot see a `down` that drops another migration's objects: it self-heals at
    // target 0 and silently loses them at the intermediate targets.
    let mut reference = LocalStore::open_in_memory()?;
    reference.migrate()?;
    let migrated_schema = schema_objects(&reference.connection)?;
    for target in 0..=latest_version() {
        let mut store = LocalStore::open_in_memory()?;
        store.migrate()?;
        store.migrate_down_to(target)?;
        store.migrate()?;
        assert_eq!(
            store.schema_version()?,
            latest_version(),
            "rolling back to {target} and migrating up must rebuild the schema"
        );
        assert_eq!(
            schema_objects(&store.connection)?,
            migrated_schema,
            "rolling back to {target} and migrating up must rebuild exactly the migrated \
             schema; a down that drops another migration's objects loses them"
        );
    }
    // Rolling back all the way removes every local-state table.
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.migrate_down_to(0)?;
    for table in TABLES {
        assert!(
            !table_exists(&store.connection, table)?,
            "table {table} must be gone after migrate_down_to(0)"
        );
    }
    assert_eq!(store.schema_version()?, 0);
    Ok(())
}

#[test]
fn migrations_directory_matches_embedded_migrations() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut up = BTreeSet::new();
    let mut down = BTreeSet::new();
    for entry in std::fs::read_dir(&directory).expect("migrations directory") {
        let file_name = entry.expect("migrations directory entry").file_name();
        let file_name = file_name.to_str().expect("migration file names are UTF-8");
        if file_name.ends_with(".up.sql") {
            up.insert(migration_identity(file_name, ".up.sql"));
        } else if file_name.ends_with(".down.sql") {
            down.insert(migration_identity(file_name, ".down.sql"));
        } else {
            panic!("unexpected file in migrations/: {file_name}");
        }
    }
    assert_eq!(up, down, "every *.up.sql must ship a matching *.down.sql");
    let registered: BTreeSet<(i64, String)> = MIGRATIONS
        .iter()
        .map(|migration| (migration.version, migration.name.to_string()))
        .collect();
    assert_eq!(
        up, registered,
        "migrations/ must match MIGRATIONS: a pair that is not registered is compiled in \
         nowhere, never applies, and no test ever runs its down"
    );
}

#[test]
fn first_unknown_version_flags_interior_ledger_rows() {
    let known = [
        Migration {
            version: 1,
            name: "one",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 3,
            name: "three",
            up_sql: "",
            down_sql: "",
        },
    ];
    assert_eq!(
        first_unknown_version(&[1, 2, 3], &known),
        Some(2),
        "an interior ledger row this build has no SQL for must be flagged, not only \
         versions outside the range"
    );
    assert_eq!(first_unknown_version(&[1, 3], &known), None);
}

#[test]
fn first_gap_version_flags_missing_ledger_rows() {
    let known = [
        Migration {
            version: 1,
            name: "one",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 2,
            name: "two",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 3,
            name: "three",
            up_sql: "",
            down_sql: "",
        },
    ];
    assert_eq!(
        first_gap_version(&[1, 3], &known),
        Some(2),
        "a known migration that was never applied below the highest applied one is a \
         gap: re-running it would layer its DDL over a schema that already has it"
    );
    assert_eq!(
        first_gap_version(&[3], &known),
        Some(1),
        "the lowest missing version is reported"
    );
    assert_eq!(
        first_gap_version(&[1, 2], &known),
        None,
        "the highest applied version may sit below the newest known one — migrate \
         exists to apply it"
    );
    assert_eq!(first_gap_version(&[1, 2, 3], &known), None);
    assert_eq!(
        first_gap_version(&[], &known),
        None,
        "an empty ledger is not a gap"
    );
}

/// Rewrites `schema_migrations` to record exactly `versions`, as a divergent build's or a
/// hand-edited ledger looks.
fn set_ledger(store: &LocalStore, versions: &[i64]) -> Result<()> {
    store
        .connection
        .execute_batch("DELETE FROM schema_migrations")?;
    for version in versions {
        store.connection.execute(
            "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, 'divergent', 0)",
            params![version],
        )?;
    }
    Ok(())
}

#[test]
fn ledger_inconsistency_refuses_a_gap_below_the_highest_applied_version() -> Result<()> {
    let known = [
        Migration {
            version: 1,
            name: "one",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 2,
            name: "two",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 3,
            name: "three",
            up_sql: "",
            down_sql: "",
        },
    ];
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    set_ledger(&store, &[1, 3])?;
    assert!(
        matches!(
            ledger_inconsistency(&store.connection, &known)?,
            Some(StorageError::MissingMigration { version: 2 })
        ),
        "a known migration that was never applied below the highest applied one must be \
         refused: re-running it would layer its DDL over a schema that already has it"
    );
    set_ledger(&store, &[1, 2])?;
    assert!(
        ledger_inconsistency(&store.connection, &known)?.is_none(),
        "a ledger below the newest known migration is not a gap — migrate exists to \
         apply the rest"
    );
    Ok(())
}

#[test]
fn ledger_inconsistency_refuses_an_unknown_interior_version() -> Result<()> {
    let known = [
        Migration {
            version: 1,
            name: "one",
            up_sql: "",
            down_sql: "",
        },
        Migration {
            version: 3,
            name: "three",
            up_sql: "",
            down_sql: "",
        },
    ];
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    set_ledger(&store, &[1, 2, 3])?;
    assert!(
        matches!(
            ledger_inconsistency(&store.connection, &known)?,
            Some(StorageError::UnknownMigration { version: 2 })
        ),
        "the query path must flag an interior ledger row this build has no SQL for, the \
         same way `first_unknown_version_flags_interior_ledger_rows` proves the rule"
    );
    Ok(())
}

#[test]
fn migrate_refuses_ledger_versions_this_build_does_not_know() -> Result<()> {
    // Version 0 is outside the known range, which the `version < 1` check in place before
    // the membership fix already refused, so this test pins the wiring — `migrate` and
    // `migrate_down_to` surfacing `StorageError::UnknownMigration` for a ledger row they
    // have no SQL for — rather than the membership fix itself. That fix is proven on
    // `first_unknown_version` by `first_unknown_version_flags_interior_ledger_rows` and,
    // through the real query path, by `ledger_inconsistency_refuses_an_unknown_interior_version`.
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.connection.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (0, 'ghost', 0)",
        [],
    )?;
    assert!(
        matches!(
            store.migrate(),
            Err(StorageError::UnknownMigration { version: 0 })
        ),
        "migrate must refuse a ledger row it has no SQL for"
    );
    assert!(
        matches!(
            store.migrate_down_to(0),
            Err(StorageError::UnknownMigration { version: 0 })
        ),
        "migrate_down_to must refuse a ledger row it has no SQL for"
    );
    Ok(())
}

#[test]
fn history_prune_respects_retention() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let now = OffsetDateTime::now_utc();
    let old_at = from_millis(to_millis(now - SignedDuration::days(40)))?;
    let recent_at = from_millis(to_millis(now))?;
    store
        .history()
        .insert(&history_entry("w", old_at, "https://old.example/"))?;
    store
        .history()
        .insert(&history_entry("w", recent_at, "https://recent.example/"))?;
    assert_eq!(
        store.history().prune_older_than(30)?,
        1,
        "the 40-day-old entry is past the 30-day retention window"
    );
    assert!(store.history().list_by_day("w", old_at.date())?.is_empty());
    let kept = store.history().list_by_day("w", recent_at.date())?;
    assert_eq!(kept.len(), 1);
    assert_eq!(
        kept.first().map(|entry| entry.target.as_str()),
        Some("https://recent.example/"),
        "entries inside the retention window are kept"
    );
    Ok(())
}

#[test]
fn history_prune_zero_days_keeps_everything() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let ancient = from_millis(to_millis(
        OffsetDateTime::now_utc() - SignedDuration::days(400),
    ))?;
    store
        .history()
        .insert(&history_entry("w", ancient, "https://old.example/"))?;
    assert_eq!(
        store.history().prune_older_than(0)?,
        0,
        "0 disables retention (PRD REQ-HIST-02 encoding): it must never wipe history"
    );
    assert_eq!(store.history().list_by_day("w", ancient.date())?.len(), 1);
    Ok(())
}

#[test]
fn history_prune_with_u32_max_days_keeps_everything() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let old_at = from_millis(to_millis(
        OffsetDateTime::now_utc() - SignedDuration::days(40),
    ))?;
    store
        .history()
        .insert(&history_entry("w", old_at, "https://old.example/"))?;
    assert_eq!(
        store.history().prune_older_than(u32::MAX)?,
        0,
        "a retention window too long to represent keeps everything instead of panicking"
    );
    assert_eq!(store.history().list_by_day("w", old_at.date())?.len(), 1);
    Ok(())
}

#[test]
fn history_delete_is_scoped_to_one_workspace() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let at = from_millis(to_millis(OffsetDateTime::now_utc()))?;
    let mine = store
        .history()
        .insert(&history_entry("w", at, "https://mine.example/"))?;
    let theirs = store
        .history()
        .insert(&history_entry("other", at, "https://theirs.example/"))?;
    store.history().delete("w", theirs)?;
    assert_eq!(
        store.history().search("other", "theirs")?.len(),
        1,
        "an id from another workspace must not delete that workspace's row"
    );
    store.history().delete("other", theirs)?;
    assert!(store.history().search("other", "theirs")?.is_empty());
    store.history().delete("w", mine)?;
    assert!(store.history().search("w", "mine")?.is_empty());
    Ok(())
}

#[test]
fn history_search_matches_target() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let at = from_millis(to_millis(OffsetDateTime::now_utc()))?;
    store
        .history()
        .insert(&history_entry("w", at, "https://api.example.com/users"))?;
    store
        .history()
        .insert(&history_entry("w", at, "https://other.test/health"))?;
    let hits = store.history().search("w", "example")?;
    assert_eq!(
        hits.first().map(|entry| entry.target.as_str()),
        Some("https://api.example.com/users"),
        "search matches the target"
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(
        store.history().search("w", "EXAMPLE")?.len(),
        1,
        "search is case-insensitive"
    );
    assert!(store.history().search("w", "no_such_target")?.is_empty());
    assert!(
        store
            .history()
            .search("other-workspace", "example")?
            .is_empty(),
        "history search is scoped to one workspace"
    );
    assert!(
        store.history().search("w", "%")?.is_empty(),
        "LIKE wildcards in the query match literally"
    );
    Ok(())
}

#[test]
fn history_orders_same_millisecond_entries_by_id() -> Result<()> {
    // Contract pin, not a discriminating proof. `created_at` ties were unordered by SQL
    // and came out id-descending only as a side effect of how SQLite serves both queries
    // from `history_workspace_created_at` (verified at 4 and at 50 000 rows), so the
    // pre-fix order matched this assertion and removing `, id DESC` would not fail here
    // either. What it pins is the order callers may rely on: two sends in the same
    // millisecond keep a total order, and a plan or schema change that reintroduces an
    // unspecified one is caught.
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let at = from_millis(to_millis(OffsetDateTime::now_utc()))?;
    let first = store
        .history()
        .insert(&history_entry("w", at, "https://first.example/"))?;
    let second = store
        .history()
        .insert(&history_entry("w", at, "https://second.example/"))?;
    let third = store
        .history()
        .insert(&history_entry("w", at, "https://third.example/"))?;
    let by_day: Vec<i64> = store
        .history()
        .list_by_day("w", at.date())?
        .into_iter()
        .filter_map(|entry| entry.id)
        .collect();
    assert_eq!(
        by_day,
        [third, second, first],
        "list_by_day: newest first, ties broken by id"
    );
    let by_search: Vec<i64> = store
        .history()
        .search("w", "example")?
        .into_iter()
        .filter_map(|entry| entry.id)
        .collect();
    assert_eq!(
        by_search,
        [third, second, first],
        "search lists same-millisecond entries in the same order as list_by_day"
    );
    Ok(())
}

#[test]
fn kv_roundtrip() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.kv().insert("dismissed.banner", "true")?;
    assert_eq!(
        store.kv().list()?,
        vec![KeyValue {
            key: "dismissed.banner".to_string(),
            value: "true".to_string()
        }]
    );
    store.kv().insert("dismissed.banner", "false")?;
    assert_eq!(
        store.kv().list()?.first().map(|flag| flag.value.as_str()),
        Some("false"),
        "insert replaces an existing flag"
    );
    store.kv().delete("dismissed.banner")?;
    assert!(store.kv().list()?.is_empty());
    Ok(())
}

#[test]
fn open_tabs_roundtrip_preserves_order() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    // Insert out of order: the list must come back in tab order, not insert order.
    for (position, item_id) in [(1_i64, "tab-b"), (0, "tab-a"), (2, "tab-c")] {
        store.open_tabs().insert(&OpenTab {
            workspace_id: "w".to_string(),
            position,
            item_id: item_id.to_string(),
            pinned: position == 0,
            draft_json: Some(serde_json::json!({ "url": item_id })),
        })?;
    }
    let tabs = store.open_tabs().list("w")?;
    let order: Vec<&str> = tabs.iter().map(|tab| tab.item_id.as_str()).collect();
    assert_eq!(order, ["tab-a", "tab-b", "tab-c"]);
    assert_eq!(
        tabs.first().map(|tab| tab.pinned),
        Some(true),
        "the pinned flag round-trips"
    );
    assert_eq!(
        tabs.get(1).and_then(|tab| tab.draft_json.clone()),
        Some(serde_json::json!({ "url": "tab-b" })),
        "the draft round-trips"
    );
    store.open_tabs().delete("w", "tab-b")?;
    assert_eq!(store.open_tabs().list("w")?.len(), 2);
    Ok(())
}

/// Contract pin, not a discriminating proof. It pins the total order of equal positions,
/// but it cannot fail on `ORDER BY position` alone: SQLite serves that query from the
/// table's primary-key index (`workspace_id`, `item_id`), so ties already came out in
/// `item_id` order before `, item_id` was written down — inserting tab-b, tab-a, tab-c at
/// one position returned tab-a, tab-b, tab-c on the pre-fix query, and deleting
/// `, item_id` leaves this test green. The pre-fix order was only accidentally stable;
/// what this test buys is that the order is a stated contract a later plan or schema
/// change cannot silently regress.
#[test]
fn open_tabs_list_breaks_position_ties_by_item_id() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    for item_id in ["tab-b", "tab-a"] {
        store.open_tabs().insert(&OpenTab {
            workspace_id: "w".to_string(),
            position: 1,
            item_id: item_id.to_string(),
            pinned: false,
            draft_json: None,
        })?;
    }
    let order: Vec<String> = store
        .open_tabs()
        .list("w")?
        .into_iter()
        .map(|tab| tab.item_id)
        .collect();
    assert_eq!(
        order,
        ["tab-a", "tab-b"],
        "equal positions must still list in a total order"
    );
    Ok(())
}

#[test]
fn local_values_roundtrip() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.local_values().insert(&LocalValue {
        workspace_id: "w".to_string(),
        scope_id: "env:staging".to_string(),
        key: "base".to_string(),
        value: "https://staging".to_string(),
    })?;
    store.local_values().insert(&LocalValue {
        workspace_id: "w".to_string(),
        scope_id: "global".to_string(),
        key: "retry".to_string(),
        value: "3".to_string(),
    })?;
    store.local_values().insert(&LocalValue {
        workspace_id: "w".to_string(),
        scope_id: "global".to_string(),
        key: "retry".to_string(),
        value: "5".to_string(),
    })?;
    let values = store.local_values().list("w")?;
    assert_eq!(values.len(), 2);
    assert_eq!(
        values.first(),
        Some(&LocalValue {
            workspace_id: "w".to_string(),
            scope_id: "env:staging".to_string(),
            key: "base".to_string(),
            value: "https://staging".to_string()
        }),
        "values list in scope, key order"
    );
    assert_eq!(
        values.get(1).map(|value| value.value.as_str()),
        Some("5"),
        "insert replaces an existing value"
    );
    store.local_values().delete("w", "global", "retry")?;
    assert_eq!(store.local_values().list("w")?.len(), 1);
    assert!(
        store.local_values().list("other")?.is_empty(),
        "values are scoped to one workspace"
    );
    Ok(())
}

#[test]
fn layout_roundtrip() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let layout = Layout {
        workspace_id: "w".to_string(),
        json: serde_json::json!({ "dock": { "width": 320 } }),
    };
    store.layout().insert(&layout)?;
    store.layout().insert(&Layout {
        workspace_id: "other".to_string(),
        json: serde_json::json!({}),
    })?;
    assert_eq!(
        store.layout().list()?,
        vec![
            Layout {
                workspace_id: "other".to_string(),
                json: serde_json::json!({})
            },
            layout.clone()
        ],
        "layouts list per workspace"
    );
    store.layout().delete("w")?;
    assert_eq!(store.layout().list()?.len(), 1);
    Ok(())
}

#[test]
fn cookies_roundtrip() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let cookie = Cookie {
        workspace_id: "w".to_string(),
        domain: ".example.com".to_string(),
        path: "/".to_string(),
        name: "session".to_string(),
        value: "abc".to_string(),
        attributes_json: serde_json::json!({ "secure": true }),
    };
    store.cookies().insert(&cookie)?;
    store.cookies().insert(&Cookie {
        value: "def".to_string(),
        ..cookie.clone()
    })?;
    let cookies = store.cookies().list("w")?;
    assert_eq!(cookies.len(), 1);
    assert_eq!(
        cookies.first(),
        Some(&Cookie {
            value: "def".to_string(),
            ..cookie.clone()
        }),
        "insert replaces the cookie with the same key"
    );
    store
        .cookies()
        .delete("w", ".example.com", "/", "session")?;
    assert!(store.cookies().list("w")?.is_empty());
    Ok(())
}

#[test]
fn recent_workspaces_roundtrip() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let older = from_millis(1_700_000_000_000)?;
    let newer = from_millis(1_700_000_100_000)?;
    store.recent_workspaces().insert(&RecentWorkspace {
        path: "/tmp/b".to_string(),
        opened_at: older,
    })?;
    store.recent_workspaces().insert(&RecentWorkspace {
        path: "/tmp/a".to_string(),
        opened_at: newer,
    })?;
    let recent = store.recent_workspaces().list()?;
    let paths: Vec<&str> = recent.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["/tmp/a", "/tmp/b"], "most recently opened first");
    assert_eq!(
        recent.first().map(|entry| entry.opened_at),
        Some(newer),
        "opened_at round-trips"
    );
    store.recent_workspaces().delete("/tmp/a")?;
    assert_eq!(store.recent_workspaces().list()?.len(), 1);
    Ok(())
}
