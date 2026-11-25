# Promotion brief: `release/pustman-v1-4` (M2)

## 1. Window
| | |
|---|---|
| Tag candidate | `release/pustman-v1-4` at **`37d4d2a`** (`S48: M2 sign-off — GO (human)`) on `pustman-v1/main` |
| Range | `release/pustman-v1-3` (`84cd1c0`) .. `37d4d2a`: 60 commits. 34 `S<N>:` steps (S39a–S47 incl. driver-added S41d/S43w/S43x, S48u1–u7, S48), 14 `log:`, 7 `progress:`, 4 `fix:` (`6694ec0` `66bb0be` `5c91e91` `7b9ad16`), 1 `build:` (`237f299`) |
| Size | 273 files, +50 903 / −403 (`crates/`: 182 files, +49 911 / −374). New crates `collections_panel`, `git`. New workspace dep `sha2`; dev profile `debug = "line-tables-only"` (`237f299`) |
| Tripwire | `TRIPWIRE 0` at `84cd1c0` and at `37d4d2a` (both re-run by the brief writer from `git archive`) |
| Migrations / ADRs / deltas | No SQL migration, no ADR, no CI-workflow change. `DEPLOY_DELTAS.md` unchanged in the window |
| CI | `839d378` run 45596754430 success. `37d4d2a` run 45599922935 **in_progress** when this was written |
| Date | 2026-11-25 (window 2026-11-23 → 2026-11-25) |
| **Verdict** | **NOT PROMOTABLE: flag rate 30/34 = 88% against a 20% ceiling (§5); promote-check item 4 will FAIL (2 files); window cap exceeded (34 unpromoted steps ≥ 16, progress.md:31)** |

## 2. Changes by module
- **collections_panel** (new): virtualized tree, keyboard nav (S39a `cf5a6eb`, S39b `8294bc9`), docked (S39w `9b35d8a`); CRUD (S40a `79a9521`), drag reorder/move (S40b `ecd7503`), watcher feeds the tree (S42a `beef20a`), problem badges (S42b `034af7a`); Postman collection import (S45a `cb35c96`), env/globals import (S45b `1be3d27`), export (S45c `2eb7a8a`); New Collection form (S48u1 `f71214b`).
- **workspace**: open/create (S41a `8b4c87a`), Open recent (S41b `ff049d0`), newer-format read-only banner (S41c `f388703`); title bar + menus + switcher (S48u2 `ff867b7`), icon rail + welcome (S48u3 `d4ae666`), console strip (S48u5 `125afd6`), New Workspace form (S48u1), `action_window` fallback + runtime native-menu flag (`66bb0be`).
- **request_editor**: Ctrl-S save (S40c `2cfddee`), read-only wiring (S41d `0096934`), "Changed on disk" bar (S42c `0cbbdf6`), breadcrumb/split Send/variable line/two-column Auth (S48u4 `ab397af`).
- **environment_picker**: env editor shared/local values (S43a `aa1bbde`), secrets to keychain (S43b `b4c5996`), Environments panel (S48u3), env menu + variable popover (S48u6 `9cb4eb1`). **response_viewer**: Set-secret panel (S43c `787963a`), response header (S48u5). **console**: error rows (S48u6).
- **git** (new): status (S44a `4cf35b7`), branch item (S44b `0e6d15d`, S44w `442e0e6`). **command_palette**: quick open (S46 `01600e4`). **ui**: FormField, disabled button, `KeyBinding::from_keymap` (S48u1, S48u7 `0b15ed2`).
- **pustman binary**: env/globals/keychain wired, installation-keyed (S43w `576e4b8`); live stored-keys cell for Resend (S43x `015083f`). **integration_tests**: sharing AC, two clones (S47 `578a8d0`).

## 3. Deploy deltas
| Step | Type | What | Status |
|---|---|---|---|
| — | — | none in window | — |

All 34 records say `Deploy delta: none`; `git diff 84cd1c0 37d4d2a -- DEPLOY_DELTAS.md` is empty; the 5 fix/build commits declare none. **Pending: 0. Manual: 0.** Prior rows all `applied`.

## 4. Decisions workers made
- **Driver scope widenings**: S41a (`install_local_store` in the hotspot), S41b (`WorkspaceOpen::local_store` made `pub`), S41c→**S41d added**, S43c (dependency cycle broken via new action `environment::EditSecret`), S43x (live `StoredSecrets` cell in `scope_wiring.rs`), S48u4 (`scope_wiring.rs`), S45b (edits outside its Edits in `api_model`) — see each record's Decisions.
- **S43w (security-relevant)**: reviewers disagreed (LOW vs HIGH); driver keyed local values, selection and keychain entries by `pustman.json` id + SHA-256 of the canonical root, "no migration (never released)" (log.md:300). **This tag releases that key scheme**; changing it later needs a migration. Worker switched to Claude after a provider content filter; a reworded-brief retry was blocked and deleted (log.md:299).
- **Wrap-up (no human policy call)**: `pustman-v1/history-env` (`b6aeecd`) not merged after security CHANGES_REQUESTED; history tabs still don't resolve the environment (#334, log.md:326). S48u7 item 1 dropped (#333).
- **Verify superset accepted**: S43a, S43b, S44a, S45b pinned filters ran PASS 10/21/5/10 instead of the pinned counts.
- **No-test fix**: `66bb0be` `action_window` has no unit test ("could not fail without the fix", log.md:323).
- **Existing tests modified (all human-authorized per log; each checked against `git diff`)**:

| Commit | File | Change | Authorization |
|---|---|---|---|
| `442e0e6` S44w | `pustman/src/status_items_tests.rs` (pre-window) | left items 1→2 | log.md:292 |
| `787963a` S43c | `pustman_actions/src/pustman_actions_tests.rs` (pre-window) | +1 line `environment::EditSecret` | log.md:300 |
| `f71214b` S48u1 | `workspace/src/workspace_open_tests.rs` (born S41a) | `create_workspace_writes_pustman_json` drives the form; name assertion changed | log.md:317 |
| `5c91e91` | `response_viewer/src/response_area_tests.rs` (born S48u5) | "Enter" pin → platform binding text | log.md:328 |
| `7b9ad16` | `ui/src/components/key_binding_keymap_tests.rs` (born S48u7) | `Ctrl+T` → platform label | log.md:331 |
| `7b9ad16` | `workspace/src/recent_non_utf8_tests.rs` (born S48u7) | `cfg(unix)`→`cfg(target_os = "linux")`: **whole file**, so the UTF-8 test also stops running on macOS | log.md:331 |
| S45a branch (pre-merge) | `collections_panel/src/import_hostile_tests.rs` | mode `0o111`→`0o311`; not visible on main (file born in `cb35c96`); **driver edit, no human authorization recorded** | log.md:310, [S45a](../progress/pustman-v1/steps/S45a.md) |

`dock_header_tests.rs` (authorized, log.md:324) and `panel_tests.rs` are **unchanged** in the window. promote-check item 4 sees only the first two rows.

## 5. Risk flags
**Flag rate = 30/34 = 88% > 20%.** Flagged = fix cycle, test-file edit or driver decision. Unflagged: S39b, S39w, S42b, S48. Flagged: S39a S40a S40b S40c S41a S41b S41c S41d S42a S42c S43a S43b S43c S43w S43x S44a S44b S44w S45a S45b S45c S46 S47 S48u1–u7 (reasons: Appendix). With the 4 `fix:` commits: 34/38 = 89%.
- **Secrets / keychain writes**: S43a–c, S43w, S43x, S45b, S48u4, S48u6 (HIGH "Add to" overwrote a disabled var, fixed). **Trust boundary (untrusted input)**: S45a, S45b imports; S47 crafted clone; S43w keying. **Deletes / on-disk writes**: S40a, S40b (rollback), S40c, S42c (HIGH conflict overwrite, fixed), S45a rollback. **Persisted format**: S48u5 `DockLayout.collapsed`; S48u7 `\0base64:` recent paths; S43w key scheme.
- **Process**: protect-hook bypass by a worker's Python script for `7b9ad16`; S48u1 and `5c91e91` edits "may have been made the same way (not checked)" (log.md:331). Pi builds ran with project trust off, so the hook never ran for them (log.md:277). The hook has no authorized-edit path ([S48](../progress/pustman-v1/steps/S48.md)).
- **Open**: #332, #333, #335 (LOW UX follow-ups), #334 (history tabs: no environment); all `open` (REST API, 2026-11-25). Branch `pustman-v1/history-env` unmerged.
- **Deep-read first**: S43w `576e4b8`, S43x `015083f`, S45a `cb35c96`, S45b `1be3d27`, S48u6 `9cb4eb1`, `66bb0be`.

## 6. 10-minute manual test
Linux: `cargo build --release -p pustman`, `./target/release/pustman`. macOS: `scripts/bundle-macos.sh`, run `target/release/Pustman.app`.
1. **First run.** Welcome screen → File › New workspace… (macOS: the **native** menu bar). Empty name and a non-empty folder are refused inline; Create writes `pustman.json`; the switcher and Open recent list it.
2. **Collections.** Sidebar "+ New" collection, add a request, Ctrl/Cmd-S; drag it into a folder; delete one. Edit its file outside the app → "Changed on disk" bar; break its JSON → badge with file:line.
3. **Secrets.** New environment with shared, local and secret values; the secret is in the OS keychain, not in any JSON. Send with an unset secret → panel → Set secret → Resend succeeds. Popover on `{{var}}` shows only the mask.
4. **Import/export.** Import a Postman collection and an environment with a `secret` var; export the collection: no secret value in the file.
5. **Read-only + git.** Open a workspace whose `formatVersion` is newer: banner, inputs disabled, env save refused. In a git repo, the status bar shows the branch. Ctrl/Cmd-P quick open.
6. **Known gap.** Open a History entry: its `{{vars}}` do not resolve (#334). Console strip expands; an error row shows `Authorization` redacted.

## 7. Not verified
1. **Native macOS menu bar**: never exercised by a test (`66bb0be` keeps tests on the in-window bar everywhere) and no macOS run is recorded ([S48](../progress/pustman-v1/steps/S48.md)). **Screen-reader output**: not checked anywhere.
2. **No Linux or macOS manual smoke of any release build**: -2 and -3 are still pending (progress.md:62, 66). S48's GO rests on Xvfb screenshots the driver inspected; the brief writer built nothing and re-ran no Verify. The `VERIFY OK` claims in records are driver prose.
3. **Fixes never re-reviewed**: S48u1, S48u2, S48u3 (CHANGES_REQUESTED), S48u4 (HIGH), S48u5, S48u7 end at "→ fix 1"; S45a's driver test fix came after the last review. **Driver-only review**: S39w, S41d, S44w; driver re-reads for S39a, S44a; no review at all for `66bb0be`, `6694ec0`, `5c91e91`, `7b9ad16`, or the S44a/S44b driver fixes.
4. Whether the S48u1 and `5c91e91` test edits bypassed the hook (log.md:331, "not checked"); edits inside step branches before squash are invisible to this diff.
5. `unlistable_directory_is_kept_and_reported` skips itself under root (`import_hostile_tests.rs:1148`); coverage depends on a non-root runner.
6. CI at `37d4d2a` still in progress. S47 flaked 5/8 before its fix (log.md:311); a 24/24 rerun count is the only stability evidence.
7. Keyboard reach and accessible labels for the S48u1–u7 UI (ux-checklist): no record names the checklist items it verified.
8. S43w CARRY-FORWARD: history, cookies and restored tabs still use the legacy `WORKSPACE_ID` ([S43w](../progress/pustman-v1/steps/S43w.md)). The progress "Window" line (progress.md:66) is stale ("unpromoted passed steps: 0").

---

## Appendix
**Flag reasons.** Fix cycles: S39a(1) S40a(2) S40b(2 + driver test fix) S40c(1) S41a(1) S41b(1) S42a(1) S42c(1) S43a(2) S43b(3) S43c(3) S43w(4) S44a(1 + driver fix) S45a(3 + driver test fix) S45b(3) S45c(1) S46(1) S47(2) S48u1–u7(1 each). Driver decisions: S41a, S41b, S41c, S41d (driver-added), S43c, S43w, S43x (driver-added, BLOCKED → widened), S44b (driver fix after review), S45b, S48u1–u7 (driver-added from the human's UX review, log.md:316). Test edits: S43c, S44w, S48u1.

**Unflagged step reviews.** S39b, S42b gpui-ui-reviewer PASS_WITH_ISSUES; S39w driver read PASS; S48 human GO at `839d378`.

**Tests.** 95 test files added. Pre-window test files modified: 2. Window-born test files later modified: 4 (§4). None deleted.

**LOWs filed in window.** #289–#335 (per records and log.md:280–329).
