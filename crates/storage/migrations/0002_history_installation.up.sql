-- 0002_history_installation: the installation key (PRD §6.1; issue #334) of the folder
-- each new history row was sent from — `pustman::scope_wiring::workspace_key`, the
-- `pustman.json` id bound to the canonical folder. Nullable: rows recorded before this
-- migration, and sends from a tab bound to no folder, have none. Not a secret: it is
-- the same key local values are stored under. The down (0002_history_installation.down.sql)
-- drops the column again.

ALTER TABLE history ADD COLUMN installation_key TEXT;
