//! Permanent discriminating test for the DDL-folding property of the schema comparison.

use super::*;
use std::collections::BTreeSet;

/// Every schema object a database holds, as `type:name` plus the DDL that created it —
/// the comparison `every_migration_has_working_down` diffs with. `type:name` alone cannot
/// see a `down` that drops a *column* of another migration's table: the table still
/// exists, so the object set matches while the column and its data are gone. Mirrors the
/// helper in `storage_tests.rs`, which this lane may not edit.
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

#[test]
fn schema_comparison_sees_one_dropped_column() -> Result<()> {
    let mut full = LocalStore::open_in_memory()?;
    full.migrate()?;
    let mut reduced = LocalStore::open_in_memory()?;
    reduced.migrate()?;
    reduced
        .connection
        .execute("ALTER TABLE history DROP COLUMN method", [])?;

    assert_ne!(
        schema_objects(&full.connection)?,
        schema_objects(&reduced.connection)?,
        "folding each object's DDL into the comparison must see a single dropped column: \
         `type:name` alone reports the same tables on both sides and lets a down that \
         drops a column of another migration's table pass the schema diff"
    );
    Ok(())
}
