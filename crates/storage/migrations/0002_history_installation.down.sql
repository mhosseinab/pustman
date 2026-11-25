-- 0002_history_installation down: exactly reverses 0002_history_installation.up.sql.
-- Rows keep everything else; with the key gone they open unbound, as before #334.

ALTER TABLE history DROP COLUMN installation_key;
