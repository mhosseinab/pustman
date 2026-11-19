# Architecture map — Pustman

The map, not the manual. Maintained by the loop; every edit ships in the same commit as the code
it describes; `doc-truth-checker` samples claims against code at every promotion. If this file and
the code disagree, the code is right and this file is a bug.

> **Target — crates appear as steps land.** The layout below is the v1 target from
> `docs/pustman-v1-plan.md` §3/§5 and `docs/PRD.md` §9. **Landed (S0–S10):** `api_model`,
> `variables`, `workspace_store`, `storage`, `secrets`. **Pending (S11–S48):** every other v1 crate
> below. **Later milestones:** `websocket_client` (M4), `grpc_client` (M5), `mqtt_client` (M6).
> Each step updates this file in the same commit as the code it adds.

## Modules and boundaries

| Module | Owns | Must not import | Entry points |
| --- | --- | --- | --- |
| `pustman` *(S11–S48)* | binary: `main.rs`, init order, window setup, app menus | — (top) | `crates/pustman/src/main.rs` |
| `api_model` | Request/Collection/Folder/Environment/Variable/Auth types; Postman v2.1 import/export; the one `redact()` (D8) | IO, `gpui` | `api_model::redact` |
| `variables` | `{{var}}` resolution, scopes (narrowest wins), dynamic vars, cycle detection | `gpui` | `variables::resolve` |
| `workspace_store` | on-disk workspace format (PRD §6.1), deterministic atomic writes, file watcher, conflict detection, item ↔ file mapping | `gpui`, UI crates | `workspace_store::{open, save_item, watch}` |
| `storage` | SQLite local store (rusqlite, D6): history, tabs, layout, local values, cookies, kv | `gpui`, UI crates | `storage::LocalStore` |
| `secrets` | OS keychain via `SecretStore` (`KeyringStore`, `MemorySecretStore` for tests) | `gpui`, UI crates | `secrets::SecretStore` |
| `git` *(S11–S48)* | thin `git` CLI wrapper (`smol::process`): status, diff, commit, pull, push, branch | `gpui` | `git::*` |
| `console` *(S11–S48)* | console entries, ring buffer, HAR export, redaction | `gpui` | `console::*` |
| `http_engine` [GPUI] *(S11–S48)* | HTTP/GraphQL/SSE transport (reqwest on `gpui_tokio`), auth helpers, cookies, TLS, redirects, timing | — | `http_engine::Transport` |
| `pustman_actions` *(S11–S48)* | every PRD action + default keymaps (D10); feature crates implement handlers | UI implementation crates | `pustman_actions::*` |
| `theme`, `settings`, `ui`, `workspace`, `text_input` *(S11–S48)* | design system, theming, typed settings, shell (Pane/Dock/ModalLayer), text editing | domain crates never import these | `pub fn init(cx: &mut App)` per crate |
| `collections_panel`, `request_editor`, `response_viewer`, `environment_picker`, `command_palette`, `history_panel` *(S11–S48)* | the Postman screens | each other (coordinate through `workspace`) | `init(cx)` |
| `integration_tests` *(S37, S47)* | end-to-end proofs (redaction test S37, sharing AC S47) | — | `crates/integration_tests/tests/` |
| later milestones | `websocket_client` (M4), `grpc_client` (M5), `mqtt_client` (M6) — same `SessionEvent` shape (PRD §9) | UI crates | `connect()` per crate |

Init order in `main.rs`: `settings → theme → ui → workspace → feature crates`.

## Data flow (the flows that matter)

1. **Send:** URL bar → `variables::resolve` → `http_engine::Transport::send` → response →
   `response_viewer` + a `console` entry via `api_model::redact`. Failure: typed `TransportError`
   (plan §5) → toast + Console row. Idempotency: one `pending_send` Task per tab; replacing it
   cancels the previous request.
2. **Save:** edit → `workspace_store::save_item` (temp file + fsync + rename; deterministic JSON)
   → `git diff` shows exactly one changed file (REQ-WSP-02).
3. **External change:** `workspace_store::watch` → tree/tab update within 1 s; unsaved edits kept
   and a "Changed on disk" bar appears (REQ-COLLAB-01).
4. **History:** response → `redact()` → `storage::LocalStore` (SQLite); secret values live only in
   `secrets` (keychain) and never in workspace files (REQ-WSP-06).
5. **Import/export:** Postman v2.1 JSON → `api_model` → workspace files; scripts preserved verbatim,
   never run (REQ-IO-06). v2.1 is the import/export format only — the on-disk format is PRD §6.1.

## Invariants (each enforced somewhere — name where)

- No `.unwrap()`/`.expect(`/`std::process::Command`/`serde_json::from_reader`/`let _ =`/
  `smol::Timer`/`std::thread::sleep`/inline `mod tests {` in non-test `crates/*/src` —
  `scripts/pustman-v1-tripwire.sh`, baseline 0, also run inside `scripts/pustman-v1-verify.sh`.
- Workspace files contain no secrets and no local state (REQ-WSP-06) — `secrets` + the S37 redaction
  test (`scripts/pustman-v1-test.sh integration_tests redaction`).
- Deterministic JSON and atomic writes (REQ-WSP-02) — `workspace_store` tests.
- Test placement (D9): `#[cfg(test)] #[path = "<file>_tests.rs"] mod tests;`, integration tests in
  `crates/<c>/tests/` — enforced by the tripwire and `.claude/hooks/protect-steps-and-tests.sh`.
- Existing tests and `docs/pustman-v1-implementation-steps.md` are read-only — enforced by
  `.claude/hooks/protect-steps-and-tests.sh` (blocks `Edit|Write|NotebookEdit` on them once they
  are on `pustman-v1/main`). `scripts/promote-check.sh` item 4 backs this up for TEST FILES only:
  it diffs `TEST_PATHS` and refuses a window that modified one.
- Errors reach the user (toast/inline error, never only logged) — code review + `security-reviewer`/
  `test-integrity-reviewer` lenses.

## Risk areas (where a change needs a flagged review)

- Secret handling and redaction (`api_model/redact`, `secrets`, `console`, history) — `security-reviewer`;
  the S37 redaction test is the proof.
- SQLite schema (`crates/storage/migrations`) — every migration ships a `down` and runs on a scratch
  DB (`sqlite3 /tmp/pustman-scratch.db`); `migration-reviewer`.
- Hotspots (`crates/pustman/src/main.rs`, `crates/pustman/Cargo.toml`, `Cargo.lock`) — serialized by
  the driver; `Cargo.lock` is regenerated at merge, never hand-merged.

## How to debug at 02:00

- Entry point: `crates/pustman/src/main.rs` (init order above). First place to look: the Console
  (`console` crate) — every send/error lands there. First command: `cargo run -p pustman`.
- Harness: `scripts/pustman-v1-init.sh` (toolchain + GPUI deps), `scripts/pustman-v1-test.sh <crate>
  [filter]` (fast loop), `scripts/pustman-v1-verify.sh` (merge gate), `scripts/pustman-v1-tripwire.sh`.
- "Staging" is the build host: `scripts/pustman-v1-init.sh --require-gpui` → `cargo build --release
  -p pustman` → manual smoke on Linux + macOS. Rollback: rebuild the previous `release/pustman-v1-*`
  tag; migrations roll back with their `down`.

## Decisions

See `docs/decisions/` — one ADR per plan §4 fork and per worker edge-case decision (ADR-0001 = D1
JSON workspace format, ADR-0002 = D6 rusqlite).
