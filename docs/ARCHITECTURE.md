# Architecture map — Pustman

The map, not the manual. Maintained by the loop; every edit ships in the same commit as the code
it describes; `doc-truth-checker` samples claims against code at every promotion. If this file and
the code disagree, the code is right and this file is a bug.

> **Target — crates appear as steps land.** The layout below is the v1 target from
> `docs/pustman-v1-plan.md` §3/§5 and `docs/PRD.md` §9. **Landed (S0–S25):** `api_model`,
> `variables`, `workspace_store`, `storage`, `secrets`, `http_engine`, `pustman_actions`, `theme`,
> `settings`, `ui`, `workspace`, `text_input`, `command_palette`, `pustman`, `console` (S32a), `history_panel` (S35a), `request_editor` (S26a), `environment_picker` (S34a), `response_viewer` (S29a), `integration_tests` (S37), `collections_panel` (S39a: the virtualized tree), `git` (S44a), `websocket_client` and `grpc_client` (S49: crates, deps and the shared rustls provider). **Pending (S26–S48):**
> every other v1 crate below (rows marked *(S26–S48)*). **Later milestones:** `websocket_client` (M4), `grpc_client` (M5), `mqtt_client` (M6).
> Each step updates this file in the same commit as the code it adds.

## Modules and boundaries

| Module | Owns | Must not import | Entry points |
| --- | --- | --- | --- |
| `pustman` | binary (`main.rs` calls `pustman::init`) + library: init order, local-store wiring, restore/autosave wiring, variable-scope wiring (`scope_wiring`: workspace globals, active environment, collection variables and the keychain into every request editor; S43w), the app-wide cookie jar (`AppCookieJar`: the transport's jar, loaded and saved through the local store, listed by Manage Cookies; S33p), window setup, app id + window icon (`app_icon`, artwork in `assets/pustman-icon-pack/`; desktop entry `scripts/install-desktop-entry.sh`, macOS bundle `scripts/bundle-macos.sh`) | — (top) | `pustman::init` (`crates/pustman/src/pustman.rs`) |
| `api_model` | Request/Collection/Folder/Environment/Variable/Auth types; Postman v2.1 import/export; the shared `redact_*` helpers (D8) | IO, `gpui` | `api_model::{redact_text, redact_headers, redact_url}` |
| `variables` | `{{var}}` resolution, scopes (narrowest wins), dynamic vars, cycle detection | `gpui` | `variables::resolve` |
| `workspace_store` | on-disk workspace format (PRD §6.1), deterministic atomic writes, file watcher, conflict detection, item ↔ file mapping | `gpui`, UI crates | `workspace_store::{open, save_item, watch}` |
| `storage` | SQLite local store (rusqlite, D6): history (each row since migration 0002 records the installation key of the folder it was sent from, #334), tabs, layout, local values, cookies, kv | `gpui`, UI crates | `storage::LocalStore` |
| `secrets` | OS keychain via `SecretStore` (`KeyringStore`, `MemorySecretStore` for tests) | `gpui`, UI crates | `secrets::SecretStore` |
| `git` | thin `git` CLI wrapper (`smol::process`): `status` (S44a: branch + dirty count; `Ok(None)` only outside a repository or without `git`, any other failure is `GitError`; inherited `GIT_DIR`-style variables removed, `LC_ALL=C`, no optional locks); `status_item` (S44b, behind the default-on `gpui` feature: the status-bar branch and dirty count for the window's opened workspace, refreshed on watcher events and on focus, an error shown inline); diff, commit, pull, push, branch *(S44w–S48)* | `gpui` | `git::*` |
| `console` [GPUI] | console entries in a memory-only ring buffer (`ConsoleStore` global, cap from `console_max_entries`, pause, clear); the send feed (`console::{record_outcome, record_error, watch}`: one redacted entry per send, S32b); text export (`console::export_text`, re-redacted, S32c); the bottom-dock panel (`ConsolePanel`: level/protocol/text filters, clear, pause, keyboard-expandable raw rows, S32d) | other feature UI crates (coordinate through `workspace`) | `console::init`, `console::ConsoleStore`, `console::record_outcome`, `console::ConsolePanel` |
| `http_engine` [GPUI] | HTTP/GraphQL/SSE transport (reqwest on `gpui_tokio`), auth helpers, cookies, TLS, redirects, timing | — | `http_engine::Transport` |
| `pustman_actions` | every PRD action + default keymaps (D10); feature crates implement handlers | UI implementation crates | `pustman_actions::*` |
| `theme`, `settings`, `ui`, `workspace`, `text_input` | design system, theming, typed settings, shell (Pane/Dock/ModalLayer, status bar, toasts, client-side title bar and resize frame; S48u2: the always-visible app title bar draws the in-window menu bar (the one menu model is `pustman::app_menus`, installed with `cx.set_menus` — native on macOS, read back with `cx.get_menus()` elsewhere), the workspace switcher (`recent_workspaces` global — S48u7: the one recent list for the title bar, welcome and palette, generation-numbered reads — `OpenRecentWorkspace`) and the quick-open search box; S48u1: `new_workspace_form` (CreateWorkspace opens a validated form; `create_named` refuses a non-empty target off the foreground); S48u5: the bottom dock rests as a one-line strip unless a saved layout says otherwise; S48u3: the dock's 48 px icon rail and the two-state welcome pane (no workspace / workspace open, Recent list), restore/autosave; `workspace_open` (S41a): open/create a workspace folder via `workspace_store`, one per window, the recent list and the launch reopen through the local store installed by `pustman::open_local_store`; `read_only` (S41c): a workspace whose format has a newer major version opens read-only with a banner, and Save shows a toast — `workspace_store::save_item` refuses such writes on its own; S41d: request editor inputs and the environment editor's fields follow `workspace::read_only::is_read_only`, and the env editor refuses before any write), text editing | domain crates never import these | `pub fn init(cx: &mut App)` (`ui`, `workspace`, `text_input`); `theme::init(cx) -> Result`; `settings::SettingsStore::init(cx, user_json) -> Result` |
| `request_editor` (S48u4: breadcrumb from the snapshot, split Send, tab count/dot badges, the variable line — value and source per `{{var}}`, secrets only as the fixed mask), `response_viewer` (S43c `set_secret`: the unresolved-secret panel's Set buttons dispatch `pustman_actions` `environment::EditSecret`, handled by `environment_picker`; Resend is offered per panel only when the failed send's own `prepare` now succeeds), `environment_picker` (picker; S48u6: the searchable environment menu and `env_editor::write` — the variable popover's writes, Add never overwrites, secrets refused, one write per scope at a time; S48u3 `environments_panel`: the rail's Environments view — list with an Active badge, click selects through the picker's `select_environment`, "＋ New" writes a numbered new environment file; S43a `env_editor`: shared values to the workspace file via `workspace_store`, local values to `storage`, a save is refused when the file changed or cannot be re-read; S43b `secret_values`: secret values live only in `SecretStore` (OS keychain), keyed by workspace/scope/variable — order: re-read check, keychain sets (each previous value kept), locals, shared file (commit point), then keychain removes; a failure rolls the sets back and every message says which part landed, never a value), `history_panel`, `collections_panel` (S39a: virtualized tree from the `WorkspaceSnapshot` following `order`, method badges, expand/collapse, unreachable items drawn as orphan roots; S39b `navigation`: arrow keys, Enter opens one tab per `ItemId`; S40a `crud`: create/rename/duplicate/delete-with-confirm through `workspace_store`, one write at a time per tree, each resolving its item and parent from a fresh on-disk read; S40b `reorder`: drag and alt-arrow moves on the same queue, a move removes the item from every container listing it, and a failed move rolls back only what it changed; S42b `problems`: problem rows show the workspace-relative path, line and reason (never file text) with a keyboard-reachable detail popover and no destructive action; S42a `external`: each window's opened workspace is watched and re-read off the foreground, one re-read per change batch, fed to the tree; closing the window or switching folders stops the watcher; S45c `export`: a collection's context menu writes Postman v2.1 from a fresh on-disk read (secret-kind values omitted), atomically, and the result lists what was skipped and which requests carry literal credentials; S45a `import`: an untrusted Postman v2.1 file read with a size bound into the open workspace through `workspace_store` — fresh ULIDs always, modelled fields never taken from unknown keys, unmappable items wrapped under `postman_raw` so they never load as live requests, scripts kept as inert text, an item cap, all-or-nothing with a rollback of exactly what it created, and a report naming skipped items and literal credentials; S45b `import_environment`: a Postman environment or globals file — secret-kind values go only to the keychain under the app's installation key (written before the file, rolled back if the file fails), keys read with `api_model::string_field` like the importer, duplicates deduped so no secret value reaches a file, globals merged with every replaced key named and refused if `pustman.json` changed meanwhile) | the Postman screens | each other, except along these edges (no cycles): `request_editor → response_viewer` (the response pane is part of a request tab), `history_panel → request_editor` (opening an entry restores a request tab), `collections_panel → request_editor` (Enter on a tree row opens or focuses the request's tab, S39b); anything else coordinates through `workspace` | `init(cx)` |
| `command_palette` | the reusable modal picker (`Picker<D: PickerDelegate>`) and the fuzzy scorer (`fuzzy::score`/`fuzzy::rank`) that S34a, S41b and S46 build their pickers on; the command palette modal (`palette.rs`, S24b); `recent.rs` (S41b): the OpenRecent picker over the recent workspaces read through `workspace::WorkspaceOpen::local_store`, newest first, Enter opens via `workspace_open`; `quick_open.rs` (S46): Ctrl/Cmd-P over every request in the opened workspace's snapshot as "Collection / Folder / Name", Enter focuses any open tab for that `ItemId` (whoever opened it) or opens one | the feature crates that depend on it (it imports `workspace` only for `ModalView`/`toggle_modal`) | `command_palette::init` |
| `integration_tests` | end-to-end proofs: redaction (S37), sharing AC with two clones of a local bare repository (S47 — the secret reaches neither A's history nor B's files, and B's send fails as UnresolvedSecret) | — | `crates/integration_tests/tests/` |
| `websocket_client` | raw WebSocket (`tokio-tungstenite`) and Socket.IO (`socketio-next`: Engine.IO 4 / Socket.IO v3–v4 servers) sessions (M4; S49 declares the crate and its deps); `tls_provider::ensure_crypto_provider` installs reqwest's rustls provider (aws-lc-rs) as the process default, once | `gpui`, UI crates | `websocket_client::tls_provider::ensure_crypto_provider` |
| `grpc_client` | gRPC over `tonic` with descriptors compiled at runtime (`protox` → `prost-reflect` dynamic messages, server reflection via `tonic-reflection`, `google.rpc.Status` details via `tonic-types`; no build.rs, no protoc) (M5; S49 declares the crate and its deps); `tls_provider::ensure_crypto_provider`, as in `websocket_client` | `gpui`, UI crates | `grpc_client::tls_provider::ensure_crypto_provider` |
| later milestones | `mqtt_client` (M6); `websocket_client` and `grpc_client` sessions — same `SessionEvent` shape (PRD §9) | UI crates | `connect()` per crate |

Init order in `pustman::init_with_store` (`crates/pustman/src/pustman.rs`): `settings → theme → ui → text_input → keymap → local store → workspace → console → feature crates (command_palette) → http client → response viewer → request editor → environment picker → history panel → restore wiring → collections panel → git branch item → variable scopes`.

Variable scopes (`crates/pustman/src/scope_wiring.rs`, S43w; #334: a History tab binds only to the folder its row was recorded in, and gets the keychain only if that folder still holds the recorded request; the module doc is the rule): the opened workspace's globals (`pustman.json` `variables`), the active environment (the picker's choice, persisted per workspace id) and each request's collection become every `RequestEditor`'s `ScopeStack`; local values come from `storage` by the winning variable's scope; secret-kind values resolve only at send, from the OS keychain (`KeyringStore`, installed as `secret_values::SecretInputs` where the local store lands) through the editor's `SecretSources`. The same state installs the environment picker's, quick-look's and environment editor's inputs. Local values, the selection and keychain entries are keyed by `scope_wiring::workspace_key` — the `pustman.json` id bound to the canonical folder it was opened from on this installation, so a copied folder carrying the same ids never reads them; a tab left open from another folder resolves nothing, and the pre-send check warns about a secret the keychain has no entry for.

## Data flow (the flows that matter)

1. **Send:** URL bar → `variables::resolve` → `http_engine::Transport::send` → response →
   `response_viewer` + a `console` entry via `api_model::{redact_text, redact_headers, redact_url}`. Failure: typed `TransportError`
   (plan §5) → toast + Console row. Idempotency: one `pending_send` Task per tab; replacing it
   cancels the previous request.
2. **Save:** edit → `workspace::Save` (Ctrl/Cmd-S; `request_editor::save`, S40c: only from a focused request editor, one write at a time per editor, a press during a write re-saves after it) → `workspace_store::save_item` (temp file + fsync + rename; deterministic JSON)
   → `git diff` shows exactly one changed file (REQ-WSP-02).
3. **External change:** `workspace_store::watch` → tree/tab update within 1 s (S42a feeds the tree; S42c `request_editor::disk_change` watches each open tab's file): a clean tab reloads silently, a dirty tab keeps its edits and shows the "Changed on disk" bar (Reload / Keep mine / Compare), a conflict-marked or invalid file is flagged on the first event and Ctrl-S refuses to overwrite it, and a rename/delete moves or drops the tab's save target (REQ-COLLAB-01).
4. **History:** response → `redact_text`/`redact_headers` → `storage::LocalStore` (SQLite); secret values live only in
   `secrets` (keychain) and never in workspace files (REQ-WSP-06).
5. **Import/export:** Postman v2.1 JSON → `api_model` → workspace files; scripts preserved verbatim,
   never run (REQ-IO-06). v2.1 is the import/export format only — the on-disk format is PRD §6.1.

## Invariants (each enforced somewhere — name where)

- No `.unwrap()`/`.expect(`/`std::process::Command`/`serde_json::from_reader`/`let _ =`/
  `smol::Timer`/`std::thread::sleep`/inline `mod tests {` in non-test `crates/*/src` —
  `scripts/pustman-v1-tripwire.sh`, baseline 0, also run inside `scripts/pustman-v1-verify.sh`.
- Workspace files contain no secrets and no local state (REQ-WSP-06) — enforced at the WRITE PATH
  by `workspace_store`'s `sanitize_secret_variables` (covered by `workspace_store` tests);
  `secrets` is where the values live (OS keychain), and the S37 redaction test
  (`scripts/pustman-v1-test.sh integration_tests redaction`) is the future end-to-end proof.
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
  `workspace_store`'s write-path stripping covers workspace files today, and the S37 redaction test
  is the end-to-end proof once it lands.
- SQLite schema (`crates/storage/migrations`) — every migration ships a `down` and runs on a scratch
  DB (`sqlite3 /tmp/pustman-scratch.db`); `migration-reviewer`.
- Hotspots (`crates/pustman/src/main.rs`, `crates/pustman/Cargo.toml`, `Cargo.lock`) — serialized by
  the driver; `Cargo.lock` is regenerated at merge, never hand-merged.

## How to debug at 02:00

- Entry point: `crates/pustman/src/main.rs` → `pustman::init` (init order above). First place to look: the Console
  (`console` crate) — every send/error lands there. First command: `cargo run -p pustman`.
- Harness: `scripts/pustman-v1-init.sh` (toolchain + GPUI deps), `scripts/pustman-v1-test.sh <crate>
  [filter]` (fast loop), `scripts/pustman-v1-verify.sh` (merge gate), `scripts/pustman-v1-tripwire.sh`.
- "Staging" is the build host: `scripts/pustman-v1-init.sh --require-gpui` → `cargo build --release
  -p pustman` → package with the app icon (`promote` skill, step 3) → manual smoke on Linux + macOS. Rollback: rebuild the previous `release/pustman-v1-*`
  tag; migrations roll back with their `down`.

## Decisions

See `docs/decisions/` — one ADR per plan §4 fork and per worker edge-case decision (ADR-0001 = D1
JSON workspace format, ADR-0002 = D6 rusqlite).
