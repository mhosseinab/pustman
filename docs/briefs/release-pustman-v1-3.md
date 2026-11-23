# Promotion brief: `release/pustman-v1-3` (M1)

## 1. Window
| | |
|---|---|
| Tag candidate | `release/pustman-v1-3` at **`430c86f`** (`progress: S38 review fixes R1-R4 merged`) on `pustman-v1/main`. Supersedes the `9e1d8c3` candidate of brief `5bbcb69` |
| Range | `release/pustman-v1-2` (`999746d`) .. `430c86f`: 207 commits, 74 `progress:`. The other 133: 47 `S<N>:` (S26a–S37 plus driver-added S33e/w/n/o/p), 4 review-fix steps `R1:`–`R4:`, 53 `remediation:`, 17 `fix:` (incl. `84803b5`), 12 others (Appendix). S38 is a sign-off with no squash commit ([S38](../progress/pustman-v1/steps/S38.md)) |
| Size | 518 files, +56 196 / −633 (`crates/`: 326 files, +51 467 / −522). Since `9e1d8c3`: 34 files, +3 227 / −161 |
| Tripwire | `TRIPWIRE 0` at `999746d` (prior brief) and at `430c86f` (re-run by the brief writer) |
| Migrations / ADRs / deltas | No migration, no ADR, no CI-workflow change. `DEPLOY_DELTAS.md` unchanged since `9e1d8c3` |
| Date | 2026-11-23 |
| **Verdict** | **NOT PROMOTABLE: flag rate 27/52 = 52% against a 20% ceiling (§5)** |

## 2. Changes by module
- **request_editor / response_viewer / console / environment_picker / history_panel** (new; S26a–S36b, S33e/n/o/p): unchanged from the prior brief (Appendix). New since: History rows can be deleted and cleared (R4 `5ae1c05`); each tab sends and cancels on its own, and auth problems warn (R2 `e77627e`); Manage Cookies confirms a domain Clear, and its saves survive closing (R3 `ade193e`).
- **api_model**: `redact` also masks percent-, form- and JSON-encoded forms of each secret; adds `percent-encoding`, `url` (R1 `522621c`).
- **http_engine**: `prepare` registers Auth-tab Bearer/API-key values as secrets (R1); one pending send per `SendOwner` (`client.rs:140`) (R2); `save_jar_latest` serializes every cookie-jar writer under the store lock (`84803b5`, #287). Earlier: S32b, S27d, S33o, `89c7df0`, `33c412c`.
- **pustman binary**: wiring (S33w, S33p); its cookie save chain now goes through `save_jar_latest` (`84803b5`). **integration_tests**: S37 plus `redaction_encoded_forms` (R1). **docs**: `docs/ARCHITECTURE.md` corrected (`682ab32`).

## 3. Deploy deltas
| Step | Type | What | Status |
|---|---|---|---|
| app-icon (`5868102`) | scripted | `scripts/install-desktop-entry.sh` | applied (build host 2026-11-20, `desktop-file-validate` clean; prose only) |

All 47 step records and S38 say `Deploy delta: none`. R1–R4 and `84803b5` declare none (no record; commit bodies). **Pending: 0. Manual: 0.**

## 4. Decisions workers made
- **S38**: M1 GO on headless evidence; Linux/macOS smoke deferred to this tag ([S38](../progress/pustman-v1/steps/S38.md)). **S33e**: `{{base}}` resolution proven only by `pipeline_tests::`. **S33o**: edit set extended into `http_engine/src/client.rs`. **S33a/S27d**: see Appendix.
- **Human, R3**: chose a confirm over undo for domain Clear (log.md:260). **R1**: accepted that `\uXXXX` and double-JSON-escaped secrets stay unmasked, and that a short Auth value (e.g. `test`) over-masks bodies (#288). **R2**: same-tab resend still replaces its send (REQ-HTTP-12); closing a tab cancels its send.
- **Existing tests modified (human-authorized)**: `a193c5c` `crates/http_engine/src/reqwest_transport_tests.rs` (+3: `.no_proxy()`, #269, log.md:229); `ade193e` `crates/response_viewer/src/cookie_dialog_clear_domain_tests.rs` (+9: the `clear_domain` helper confirms; no assertion changed; log.md:260). Both are promote-check item-4 exceptions. Also `affc474` rewrote the window-born `open_entry_tests.rs` (log.md:238).

## 5. Risk flags
**Flag rate = 27/52 = 52% > 20%.** Method as before: denominator = 47 `S<N>:` steps, now plus the plan/fix steps R1–R4 and `84803b5` (S38, remediation lanes and `docs:` excluded). Flagged: 22 S-steps (Appendix) + R1 (secrets/redaction), R2 (auth, send/cancel ownership; my call), R3 (deletes, persistence), R4 (deletes) + `84803b5` (persistence). Adding the other 16 `fix:` commits (3 flagged): **30/68 = 44%**.

### Review fixes (independent deep-read reviews, log.md:259–261)
| Review | Found | Fixed by |
|---|---|---|
| Redaction | HIGH encoded secret forms leak to Console, history, export; MEDIUM Auth-tab values not registered; MEDIUM S37 can't catch either | R1 `522621c` (+8 tests) |
| Send pipeline | MEDIUM one global `HttpClient`: a tab's send cancels another's; MEDIUM unresolved auth vars unwarned; MEDIUM incomplete/unsupported auth silent; TLS opt-out clean | R2 `e77627e` (+5 test files) |
| Deletes | MEDIUM cookie Clear-domain has no confirm; MEDIUM History delete/clear never wired (REQ-HIST-04 unmet) | R3 `ade193e`, R4 `5ae1c05` |
| S33o + `affc474` | PASS_WITH_ISSUES (LOWs) | → #288 |
| S33p LOWs (#287) | dialog saves off the chain; stale first load | `84803b5` |

**Deep-read first:** R2 `e77627e`, R1 `522621c`, `84803b5`, R3 `ade193e`, S33p `475bee6`.

## 6. 10-minute manual test
Linux: `cargo build --release -p pustman`, `./target/release/pustman`. macOS: `scripts/bundle-macos.sh`, smoke `target/release/Pustman.app`.
1. **Send/cancel per tab.** Two tabs to a slow server; send both. Cancel in tab A: tab B completes. Close a sending tab: its send stops.
2. **TLS.** Self-signed host errors; "Disable verification" re-sends with the badge; other tabs stay verified.
3. **Secrets.** Bearer `ab c/1` in Auth, also in a query param. Console raw, Timeline, History row and export show no `ab%20c%2F1`/`ab+c` form.
4. **Auth warnings.** Bearer `{{missing}}` warns before send and in the Console; empty API key warns.
5. **Cookies.** Cookie set, survives relaunch, sent next time. Manage Cookies → Clear domain: confirm names domain and count; Escape cancels.
6. **History.** Live row; delete one row (button, Shift+Delete / Cmd+Backspace); Clear all, Cancel keeps rows; relaunch, no save toast.

## 7. Not verified
1. **No Linux or macOS manual smoke of the release build has run** (S38 defers it to this tag); the release build at `430c86f` was not built by the brief writer.
2. **S38 evidence was headless** (Xvfb + lavapipe, cloud container): macOS, visual judgement, keyboard reach of Max redirects "not verifiable here" (log.md:256). `bundle-macos.sh` never ran on macOS (log.md:174).
3. **No CI run for `475bee6`/`924e953`** (Actions disabled). CI at `430c86f`: run 45158916827 `in_progress` on both jobs when written. `9e1d8c3` green (run 45146195659). 2 unexplained macOS flakes of `watcher::git_dir_changes_ignored`.
4. **R1–R4 have no display or E2E check; GPUI tests only.** None has a step record: review verdicts come from commit bodies and log.md:260–261, and `.reports/` is not in this checkout. R2's final verdict after CHANGES_REQUESTED is not stated. `84803b5` had no independent review (driver RED→GREEN claim only).
5. Older gaps unchanged (Appendix): single-reviewer steps, incomplete verify runs, thin S35b/S37/S30d evidence, no records for 17 `fix:` commits.

**Open follow-ups:** #288 (R1–R4 LOWs: lost auth warnings on transport failure, `is_sending` scope, unbindable History keys, non-transactional cookie writes); #271–#285 and batches #241–#263 (human-only items #106/#107/#116/#151/#195/#236 are closed into those umbrellas; #208's redirect wording is closed as superseded by open #256); medium #31; #20.

---

## Appendix
**Flag reasons (S-steps).** S27c cURL replaces request; S27d persisted `RequestSettings`; S28b auth secrets; S28c TLS-off; S30e `Set-Cookie` `Debug` leak; S31b masks `Authorization`; S32a `Debug` URL leak (#108); S32b redacted wire text, `refused_count` (closes S3 CARRY-FORWARD, `crates/console/src/feed.rs`); S32c export re-redaction; S33c keychain reads; S33e auth/secrets/history live; S33w history writes; S33o history-row order; S33p cookie jar persisted; S34a kv write; S34b secret reveal; S34w; S35c delete/clear; S35w wires deletes; S36a cookie delete; S36b `save_jar`; S37 redaction NFR. Flagged prior `fix:` commits: `89c7df0`, `a193c5c`, `918d534`.

**Prior decisions.** S33a: a MEDIUM closed by amending the edit set. S27d: `#[allow(clippy::large_enum_variant)]` accepted. S33o: `history_panel` depends on `console` (UI→UI). LOW remediation stopped at 52/85 lanes; 92 reviewer LOWs filed as umbrellas #271–#285 (log.md:254).

**Prior Not-verified detail.** Single driver/orchestrator review: S26w, S33c review 2, S33p build. S30c verify cut off (direct `cargo test` re-run); S28b first verify exit 1. S35b names no Verify; S37/S30d no Verify re-run. No records for `fix:` commits, `5868102`, `222c810`; 52 remediation lanes have no records.

**Other non-step commits.** `5868102`, `c31e3b5`, `774b97c`, `7688dc2`, `dabaedc`, `222c810`, `0a87f87`, `8a9b913`, `a5fcbbb`, `40deef7` (merge), `5bbcb69` (prior brief), `682ab32`.

**Tests.** 206 test files added (12 since `9e1d8c3`). Pre-existing files modified: 1 (`reqwest_transport_tests.rs`). Window-born files later modified: `cookie_dialog_clear_domain_tests.rs` (born `b9127b2`, edited R3), `open_entry_tests.rs` (`affc474`). None deleted.
