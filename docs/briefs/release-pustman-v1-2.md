# Promotion brief: `release/pustman-v1-2` (M0)

## 1. Window
| | |
|---|---|
| Tag candidate | `release/pustman-v1-2` at **`999746d`** on `pustman-v1/main` |
| Range | `release/pustman-v1-1` (`6620061`) .. `999746d`. The range holds 34 `S<N>:` squash commits for plan steps S11–S24, plus S25 as a sign-off with no squash commit. It also holds 11 non-step commits, listed in the Appendix |
| After the sign-off | M0 GO was signed off at `e999232` ([S25](../progress/pustman-v1/steps/S25.md)). After that point the range adds `345a9aa` (progress files only), `019923c` (docs) and **`999746d` (a code fix, §4)** |
| Size | 324 files, +61 357 / −1 349 |
| Tripwire | `TRIPWIRE 0` at `6620061` and at `999746d`, re-run by the brief writer |
| Date | 2026-11-20 |
| **Verdict** | **NOT PROMOTABLE: the flag rate is 21/34 = 62% against a 20% ceiling (§5)** |

## 2. Changes by module
- **http_engine** (S11–S16b): transport, error taxonomy and send/cancel; reqwest with redirects and TLS toggle; variables, auth and keychain secrets; bodies, file refs, temp-file spill; cookie jar; redacted history and retention. **Not a dependency of the binary yet**: `crates/pustman/Cargo.toml` at `999746d` has no `http_engine`.
- **api_model**: redaction prefix helpers for history (S16a).
- **settings / theme**: layered settings, with polling reload that keeps the last good values (S17). Bundled light and dark themes (S18).
- **ui**: components, icons, story view (S19a–c); `keystroke_label` key hints (S22f).
- **text_input**: buffer, single-line input with IME, clipboard and undo, multi-line `TextArea` (S20a–c, S22f).
- **pustman_actions**: every PRD action plus the default keymaps (S21).
- **workspace**: docks, status bar, panes and tabs, toasts, modal layer, layout/tab restore and autosave (S22a–d, S23a–b), client title bar (S22f).
- **command_palette**: picker, fuzzy scorer, action palette (S24a–b).
- **pustman binary**: init order, DB with an in-memory fallback, startup toasts (S22e), palette (S24w), restore wiring (S23w), and `text_input::init` (`999746d`).

## 3. Deploy deltas
| Step | Type | What | Status |
|---|---|---|---|
| (none in window) | – | – | – |

Every record in the window says `Deploy delta: none`, except S18, which has no record (§7). `DEPLOY_DELTAS.md` has no row from S11 or later. The window does not touch `.github/` or `rust-toolchain.toml`. **Pending: 0. Manual: 0.**

## 4. Decisions workers made
- **S13c**: an empty keychain secret is sent as `""`. Unsupported or incomplete auth still sends ([S13c](../progress/pustman-v1/steps/S13c.md)).
- **S12**: header order is not guaranteed. **MEDIUM #31 was escalated, not fixed** ([S12](../progress/pustman-v1/steps/S12.md)). **S15b**: a single non-JSON row fails the whole cookie load (#61).
- **S23a**: the tab save is not atomic and nothing flushes it on close (#85). **S23w**: there is one `"default"` workspace key. No item kinds are registered until S26w, so tabs do not restore (#103).
- **S19a**: `units.rs` was rewritten after review found a near-copy of a GPL file. A human should judge the licensing.
- **S22f** (added by the driver): it edited 2 read-only test files so that their expected value comes from the function under test (`8771b0b`, #104).
- **`999746d`** (not a step, no record): `text_input::init` was never called, so the app ignored text-field editing keys. **The M0 smoke at `e999232` therefore ran without them.** The fix also adds `crates/pustman/tests/wire_text_input.rs`.
- **`019923c`** (doc-truth exception): an `ARCHITECTURE.md` catch-up for S11–S25 that ships separately from the code it describes, against the AGENTS.md rule that docs ship with their code.
- **GPUI pin** moved from `58849026` to `0cc1ffb6` (`fdd8325`). The read-only steps file was rewritten from 49 to 109 steps (`70fd735`). One ADR is new: [ADR-0003](../decisions/ADR-0003-timing-phases.md).

## 5. Risk flags
**The flag rate is 21/34 = 62% > 20%, so the window is NOT PROMOTABLE until the flag rules tighten or the human rules again (as for -1).** Final `PASS_WITH_ISSUES` alone already gives 9/34 = 26%. Per-step reasons are in the Appendix.
- **PASS_WITH_ISSUES:** S13b, S13c, S14b, S14c, S17, S19a, S20a, S21, S22b.
- **Auth, trust boundaries or deletes:** S11, S12, S14a, S15a, S15b, S15c, S16a, S16b, S22e, S23a, S23b, S23w.

Suggested deep-read order (a shortlist, not the rate): S16a, S12, S13c, S23a+S23w, S16b, S14b.

## 6. 10-minute manual test
Run on Linux and macOS: `cargo build --release -p pustman && PUSTMAN_STARTUP_TRACE=1 ./target/release/pustman`
1. **Startup.** A window titled "Pustman" opens with the left dock, the bottom dock and the status bar. No error toast appears, and the startup trace prints.
2. **Title bar.** Move, resize from each edge, double-click to maximize, and use Min, Max and Close. Press on the bar and release off it: the window must not stick (#105).
3. **Palette.** `Ctrl/Cmd+Shift+P` opens it with a single placeholder and `Ctrl+…` hints. **Type `consolx`, then Backspace, ←/→, Ctrl/Cmd+Z, and paste.** These keys are the `999746d` fix. Type `console` and press Enter: the bottom dock toggles. Esc closes the palette.
4. **Dock shortcuts.** `Ctrl/Cmd+\` toggles the sidebar. `Ctrl/Cmd+Alt+C` toggles the console.
5. **Layout restore.** Close a dock, wait more than 2 s, quit and relaunch. The dock state must restore. Tabs are not expected to restore (#103).
6. **Settings error.** Before launch, put `{` in `~/.config/pustman/settings.json` (on macOS `~/Library/Application Support/pustman/`). Launch: an error toast appears and the app runs. Remove the file afterwards.

## 7. Not verified
1. **`999746d` has had no human run.** Text-field editing has never been seen in the app; the M0 GO predates the fix. The only evidence is the wire test, which passed when re-run here (`cargo test -p pustman --test wire_text_input`, 1 passed). No red run was recorded, and the commit has no step record or review.
2. **No macOS smoke has run.** No artifact shows a macOS build of `pustman` ([S25](../progress/pustman-v1/steps/S25.md)).
3. **Window move, minimize, maximize and the window menu cannot be tested on the GPUI test platform (#107).** Only the human Linux smoke covered them.
4. **CI has not run on the candidate.** `origin/pustman-v1/main` = `e5347fc`, which is also the last CI run (44240566190).
5. **S18 has no step record.** `steps/S18.md` is missing, so S18 is unverified, not passed (`2fedd34`).
6. **Single driver or orchestrator review only:** S15c, S16b (S16 is on the high-stakes list), S19c, S22c, S22d, S22e, S23a, S23b, S23w, S24b and S24w. S13b's round-5 reviewer had no shell, and S13a's record states no verdict.
7. **Self-reported only:** the `(worker)` red runs and the S22e startup trace (550 / 129 ms; nobody looked at the window).
8. **The app has never sent an HTTP request.** TLS, cookies, history and spill have run only in tests.
9. **Not re-run by the brief writer:** the full `scripts/pustman-v1-verify.sh` and the release build at `999746d`.
10. **No review exists for the non-step code and config commits** `fdd8325`, `125ee03`, `70fd735` and `999746d`.
11. **Progress claims are stale.** `docs/pustman-v1-progress.md` says "unpromoted passed steps: 0", G-DEPS shows `pending`, and the S18 link and the -1 brief link (`briefs/release/…`) are dead. Windows remains untested.

---

## Appendix
**Flag reasons, one per step.**
- S11: Debug and URL secret leaks (3 MEDIUM fixed).
- S12: TLS toggle, redirect credential stripping, #31 open.
- S13b: auth applied to requests (PWI).
- S13c: keychain secrets, unresolved-secret gate (PWI).
- S14a: reads files off disk; multipart names in Debug.
- S14b: workspace-root path containment, race #35 (PWI).
- S14c: temp-file writes and unlink (PWI).
- S15a: PSL supercookie boundary; cookie delete/clear API.
- S15b: cookies persisted; PSL re-check on load.
- S15c: cookies across redirect origins.
- S16a: secrets redacted into stored history; the orchestrator both fixed round 3 and acted as second reviewer.
- S16b: history prune deletes rows.
- S17: watcher untested, #15 (PWI).
- S19a: GPL near-copy rewritten (PWI).
- S20a: no accessible label, #69 (PWI).
- S21: keymap problems had no caller, #12 (PWI).
- S22b: duplicate tab ids, #71 (PWI).
- S22e: opens and migrates the real `pustman.db`.
- S23a: non-atomic replace-by-difference writes, #85.
- S23b: autosave writes drafts.
- S23w: makes that persistence live in the app.

**Tests.** 64 test files were added. Only one commit edits an existing test file: `8771b0b`, which changes 2 files. The net window diff shows 0 deleted test lines only because those files were born inside the window.

**Non-step commits.** `839d059`, `2a18529`, `fdd8325`, `125ee03`, `80386e4`, `70fd735`, `0df6ee5`, `b52c4ba`, `4fa3e0c`, `019923c`, `999746d`.

**Issues.** 106 `pustman-v1` issues are open. The only open `medium` is #31. #12 may be stale, since S22e toasts keymap problems; this is unchecked.

**Window cap (D11).** The range covers 15 plan steps (S11–S25) against a cap of ≥16. As sub-steps, 34 commits landed. Per-step SHAs are in each record under `docs/progress/pustman-v1/steps/`.
