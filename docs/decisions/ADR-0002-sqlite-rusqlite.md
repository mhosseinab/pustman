# ADR-0002: The local store is SQLite via rusqlite, bundled (D6)

- **Status:** accepted
- **Date:** 2026-11-17
- **Source:** plan §4.D6 (PRD §6.1 REQ-WSP-06, §8.5)

## Context

History, open tabs, layout, local variable values, cookies and caches live in a per-user local store
— never in workspace files (REQ-WSP-06). The fork was `rusqlite` (synchronous, bundled SQLite) vs
`sqlx` (async, runtime-coupled). Whatever is chosen, `AGENTS.md` forbids blocking IO on the
foreground thread, and tests must run against real migrations.

## Decision

**`rusqlite` with the `bundled` feature**, behind synchronous functions called from
`cx.background_spawn`. The DB lives in the per-user data dir, keyed by workspace `id`. Migrations
live in `crates/storage/migrations/`, every migration ships a reversible `down`, and `LocalStore::
migrate` applies them on open. Secrets are **not** here: they live in the OS keychain (`secrets`
crate).

## Consequences

- No async runtime is needed for local storage; the API stays synchronous and testable with temp DBs
  (`tempfile`) that run the real migrations.
- `sqlx`'s compile-time checked queries are not available; query correctness is proven by tests
  instead (including the migration `down` on a scratch DB: `sqlite3 /tmp/pustman-scratch.db`).
- `promote-check` refuses a window whose migrations lack a `down` (item 5).
- A later step must honor: migrations are additive, each with a `down`; the SQLite file bytes are
  covered by the S37 redaction test.

## Dissent / alternatives rejected

- `sqlx` — rejected: drags an async runtime into a domain crate and complicates temp-DB tests for no
  benefit at this scale (plan §4.D6).
