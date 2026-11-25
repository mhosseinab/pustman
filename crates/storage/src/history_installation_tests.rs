//! The folder a history row was recorded in (issue #334): migration 0002's nullable
//! `installation_key` column, what is written into it, and its down step.

use super::*;

const INSTALLATION: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0.0123456789abcdef0123456789abcdef";

fn entry(target: &str) -> HistoryEntry {
    HistoryEntry {
        id: None,
        workspace_id: "workspace".to_string(),
        created_at: OffsetDateTime::now_utc(),
        protocol: "http".to_string(),
        method: "GET".to_string(),
        target: target.to_string(),
        status: Some(200),
        duration_ms: Some(1),
        request_json: serde_json::json!({ "method": "GET" }),
        response_summary_json: None,
        response_body_prefix: None,
    }
}

fn has_installation_column(connection: &Connection) -> Result<bool> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = 'installation_key'",
        [],
        |row| row.get(0),
    )?;
    Ok(count == 1)
}

#[test]
fn test_history_rows_keep_the_installation_they_were_recorded_in() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    let history = store.history();
    let keyed = history.insert_recorded_in(&entry("https://a.example.test"), Some(INSTALLATION))?;
    let unkeyed = history.insert(&entry("https://b.example.test"))?;

    let keys = history.installation_keys("workspace", "")?;
    assert_eq!(keys.get(&keyed).map(String::as_str), Some(INSTALLATION));
    assert!(!keys.contains_key(&unkeyed), "a row with no key has none");
    assert!(
        history.installation_keys("workspace", "b.example")?.is_empty(),
        "the keys follow the same search the rows do"
    );
    assert!(history.installation_keys("other", "")?.is_empty());
    Ok(())
}

#[test]
fn test_rows_recorded_before_the_migration_have_no_installation() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    store.migrate_down_to(1)?;
    store.connection.execute(
        "INSERT INTO history (workspace_id, created_at, protocol, method, target,
                              request_json)
         VALUES ('workspace', 0, 'http', 'GET', 'https://old.example.test', '{}')",
        [],
    )?;
    store.migrate()?;
    assert_eq!(store.history().search("workspace", "")?.len(), 1);
    assert!(store.history().installation_keys("workspace", "")?.is_empty());
    Ok(())
}

#[test]
fn test_installation_migration_down_drops_the_column_and_keeps_the_rows() -> Result<()> {
    let mut store = LocalStore::open_in_memory()?;
    store.migrate()?;
    assert!(has_installation_column(&store.connection)?);
    store
        .history()
        .insert_recorded_in(&entry("https://a.example.test"), Some(INSTALLATION))?;

    store.migrate_down_to(1)?;
    assert_eq!(store.schema_version()?, 1);
    assert!(!has_installation_column(&store.connection)?);
    assert_eq!(
        store.history().search("workspace", "")?.len(),
        1,
        "the down drops the key, never the row"
    );

    store.migrate()?;
    assert!(has_installation_column(&store.connection)?);
    assert!(store.history().installation_keys("workspace", "")?.is_empty());
    Ok(())
}
