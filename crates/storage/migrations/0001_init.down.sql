-- 0001_init down: exactly reverses 0001_init.up.sql (every migration ships a
-- working down — `every_migration_has_working_down`). The `schema_migrations`
-- bookkeeping table belongs to the migration runner and is not touched here.

DROP INDEX IF EXISTS history_workspace_created_at;
DROP TABLE IF EXISTS recent_workspaces;
DROP TABLE IF EXISTS cookies;
DROP TABLE IF EXISTS local_values;
DROP TABLE IF EXISTS layout;
DROP TABLE IF EXISTS open_tabs;
DROP TABLE IF EXISTS history;
DROP TABLE IF EXISTS kv;
