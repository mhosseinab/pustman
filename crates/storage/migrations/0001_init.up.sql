-- 0001_init: the local-state tables (PRD §6.1 REQ-WSP-06, §8.5; ADR-0002).
-- Local state only — secrets live in the OS keychain (`secrets` crate) and must
-- never be written here. Timestamps are unix milliseconds, UTC.

CREATE TABLE kv (
    key   TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE history (
    id                    INTEGER NOT NULL PRIMARY KEY,
    workspace_id          TEXT    NOT NULL,
    created_at            INTEGER NOT NULL,
    protocol              TEXT    NOT NULL,
    method                TEXT    NOT NULL,
    target                TEXT    NOT NULL,
    status                INTEGER,
    duration_ms           INTEGER,
    request_json          TEXT    NOT NULL,
    response_summary_json TEXT,
    response_body_prefix  BLOB
) STRICT;

CREATE INDEX history_workspace_created_at ON history (workspace_id, created_at);

CREATE TABLE open_tabs (
    workspace_id TEXT    NOT NULL,
    position     INTEGER NOT NULL,
    item_id      TEXT    NOT NULL,
    pinned       INTEGER NOT NULL,
    draft_json   TEXT,
    PRIMARY KEY (workspace_id, item_id)
) STRICT;

CREATE TABLE layout (
    workspace_id TEXT NOT NULL PRIMARY KEY,
    json         TEXT NOT NULL
) STRICT;

CREATE TABLE local_values (
    workspace_id TEXT NOT NULL,
    scope_id     TEXT NOT NULL,
    key          TEXT NOT NULL,
    value        TEXT NOT NULL,
    PRIMARY KEY (workspace_id, scope_id, key)
) STRICT;

CREATE TABLE cookies (
    workspace_id    TEXT NOT NULL,
    domain          TEXT NOT NULL,
    path            TEXT NOT NULL,
    name            TEXT NOT NULL,
    value           TEXT NOT NULL,
    attributes_json TEXT NOT NULL,
    PRIMARY KEY (workspace_id, domain, path, name)
) STRICT;

CREATE TABLE recent_workspaces (
    path      TEXT    NOT NULL PRIMARY KEY,
    opened_at INTEGER NOT NULL
) STRICT;
