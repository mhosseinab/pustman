# Promotion brief: `release/pustman-v1-3` (M1)

## 1. Window
| | |
|---|---|
| Tag candidate | `release/pustman-v1-3` at **`9e1d8c3`** (`progress: S38 M1 GO`) on `pustman-v1/main` |
| Range | `release/pustman-v1-2` (`999746d`) .. `9e1d8c3`: 197 commits, 71 of them `progress:`. The other 126: 47 `S<N>:` squash commits (S26a–S37, plus the driver-added S33e, S33w, S33n, S33o, S33p), 53 `remediation: <lane> LOWs`, 16 `fix:`, and 10 others (Appendix). S38 is a sign-off with no squash commit ([S38](../progress/pustman-v1/steps/S38.md)) |
| Size | 503 files, +53 059 / −562 (`crates/`: 312 files, +48 440 / −452) |
| Tripwire | `TRIPWIRE 0` at `999746d` and at `9e1d8c3`, both re-run by the brief writer |
| Migrations / ADRs | No file under `crates/storage/migrations` changed. No ADR added (`docs/decisions/` unchanged) |
| Date | 2026-11-23 |
| **Verdict** | **NOT PROMOTABLE: the flag rate is 22/47 = 47%, against a 20% ceiling (§5)** |

## 2. Changes by module
- **request_editor** (new; S26a, S26b, S26w, S27a–d, S28a–c, S29e, S33a–e, S33n): request tab, URL bar and method, Send/Cancel, the Params, Headers, Body, Auth and Settings tabs, cURL paste and copy, variable highlight, autocomplete, hover and the unresolved-variable warning. S33e routes Send through `http_engine::send_request`, so auth, secrets and history apply.
- **response_viewer** (new; S29a–c, S29w, S30a–e, S31a–b, S33o, S36a–b, S33p): status line, headers, Raw/Pretty/Hex/safe Preview, find, the large-body fallback, save and copy, the Cookies tab, Timeline and request-as-sent, and the Manage Cookies dialog. Human-requested fixes add text selection and copy (`143c0dd`, `affc474`, `3a625e9`).
- **console** (new; S32a–d, S32w): ring buffer, send feed, redacted export, bottom-dock panel.
- **environment_picker** (new; S34a, S34b, S34w): picker that persists the active environment; quick-look that masks secrets.
- **history_panel** (new; S35a–d, S35w, S33o): list, open, delete/clear, live refresh. History tabs get a fresh id (`918d534`).
- **http_engine**: `SendOutcome` carries the sent request (S32b); auto-header disable (S27d); the order of cancel and the history row (S33o); macOS TLS name-mismatch reason (`89c7df0`); a query param already in the URL is sent once (`33c412c`).
- **workspace_store**: on macOS the watcher canonicalizes its root (`89c7df0`). **workspace**: `restore` saves only the first tab per id (`918d534`).
- **pustman binary**: wires the editor, viewer, console, environment picker, history panel and history recording (S33w), plus one app cookie jar loaded and saved through the store (S33p). App icon (`5868102`).
- **integration_tests** (new; S37): end-to-end redaction proof. **Harness**: cleanup script, the 3-core cargo cap (`.cargo/config.toml`), verify-script diagnostics (`8caa86a`, `0ab0bf3`).

## 3. Deploy deltas
| Step | Type | What | Status |
|---|---|---|---|
| app-icon (`5868102`) | scripted | `scripts/install-desktop-entry.sh`: per-user Linux desktop entry and icons | applied (build host, 2026-11-20; prose in `DEPLOY_DELTAS.md`, no captured output) |

All 47 step records say `Deploy delta: none`, and so does S38. `.cargo/config.toml` (jobs = 3) is repo config and applies by itself. **Pending: 0. Manual: 0.**

## 4. Decisions workers made
- **S38**: the human gave M1 GO on headless evidence (Xvfb + lavapipe, a pi worker) and deferred the Linux and macOS smoke to this tag ([S38](../progress/pustman-v1/steps/S38.md)). The driver ruled that the pi worker could not record the GO itself (log.md:268).
- **S33e**: the M1 app has no variable source until S41/S43. AC1's `{{base}}` resolution is proven only by `pipeline_tests::` ([S33e](../progress/pustman-v1/steps/S33e.md)).
- **S33p**: the driver finished the build after a content-filter block and a timeout. Cookies loaded at startup replace any captured earlier. The dialog's own `save_jar` is still off the save chain, so it can race other saves (#287, open; `cookie_dialog.rs:421`) ([S33p](../progress/pustman-v1/steps/S33p.md)).
- **S33o**: the driver extended the edit set into `http_engine/src/client.rs`. `history_panel` now depends on `console`, a UI→UI dependency. Review 1's HIGH was a Verify filter the driver wrote wrong.
- **S33a**: a MEDIUM was closed by amending the edit set, with no fix cycle. **S27d**: the driver accepted `#[allow(clippy::large_enum_variant)]` in `api_model` and `workspace_store`.
- **`affc474`** (human-authored fix): it rewrote the read-only `history_panel/src/open_entry_tests.rs` "through the shell" because the test asserted the removed two-tab behaviour (log.md:251). **`a193c5c`** edits the read-only `reqwest_transport_tests.rs` (+3 lines). These are the only two edits to existing test files in the window.
- **LOW remediation** was stopped by the human at 52 of 85 lanes. The 92 LOW findings from its reviewers became umbrella issues #271–#285, not one issue per finding (log.md:267).

## 5. Risk flags
**Flag rate = 22/47 = 47% > 20%, so NOT PROMOTABLE until the flag rules tighten or the human rules again (as for -1 and -2).** The denominator is the 47 `S<N>:` squash commits. S38 (no code) and the remediation lanes are excluded. Adding the 16 `fix:` commits, of which 3 are flagged, gives 25/63 = 40%. Per-step reasons are in the Appendix.
- **Secrets / redaction:** S28b, S30e, S31b, S32a, S32b, S32c, S33c, S33e, S34b, S37.
- **Persistence:** S27d, S33w, S33o, S33p, S34a, S34w, S36b. **Deletes:** S35c, S35w, S36a. **TLS:** S28c. **Untrusted import:** S27c (pasted cURL).
- `fix:` commits: `89c7df0` (TLS, watcher), `a193c5c` (TLS test), `918d534` (tab persistence).

**Deep-read first:** S33p, S33e, S33o, S37 + S32b/S32c, S35c + S36a, S28c, `affc474`.

## 6. 10-minute manual test
Linux: `cargo build --release -p pustman`, then `./target/release/pustman`. macOS: `scripts/bundle-macos.sh`, then smoke `target/release/Pustman.app`.
1. **Send.** Ctrl/Cmd+T opens a request. POST JSON `{"name":"x"}` to an echo server. Check the status line, the Pretty body and the Console row. Esc mid-send shows "Cancelled".
2. **TLS.** Send to a self-signed host: a TLS error appears. "Disable verification for this request" re-sends and shows the badge, and other tabs stay verified.
3. **Cookies.** Send to a server that sets a cookie: it shows in the Cookies tab and in Manage Cookies. Quit and relaunch: the cookie is still there, and the next send carries it. Delete it.
4. **History.** The send appears without reopening the panel. Open it: exactly one tab. Delete one entry, then Clear; Cancel must leave history untouched. Quit and relaunch with no "could not be saved" toast.
5. **Secrets.** Put `Authorization: Bearer abc123` in Headers and send. The Console detail and the Timeline "request as sent" must mask it.
6. **Layout.** Check the Timeline, the Settings-tab number inputs, header-row checkboxes and console rows (#199–#202 are still open on the tracker).

## 7. Not verified
1. **No Linux or macOS manual smoke of the release build has run.** S38 defers it to this tag.
2. **The S38 evidence was headless (Xvfb + lavapipe in a cloud container), not on macOS.** All macOS checks, visual judgement and keyboard-only reach of Max redirects were "not verifiable here" (log.md:269). `scripts/bundle-macos.sh` has never run on a macOS host (log.md:174).
3. **CI has no run for `475bee6` or `924e953`**, because Actions was disabled at the time ([S38](../progress/pustman-v1/steps/S38.md)). For the candidate, run 45146195659 was still `in_progress` on both jobs when this brief was written. The last green macOS run in the window is on `396395c`. 2 macOS flakes of `watcher::git_dir_changes_ignored` have no known cause.
4. **Single driver or orchestrator review only:** S26w (in-driver review), S33c review 2 (scoped, driver), the S33p build (finished by the driver). The `.reports/*-review*.md` files that the records cite are not in this checkout, so the brief writer could not read the reviews themselves.
5. **Verify runs that did not complete cleanly:** after the S30c merge, verify stopped in the test stage. A direct `cargo test --workspace` re-run replaced it, with no `VERIFY OK` line. After the S28b merge, the first run exited 1 and a re-run passed.
6. **Thin review evidence:** the S35b record names no Verify command. The S37 and S30d records show probes but no Verify re-run.
7. **No step record or review exists for the 16 `fix:` commits or for `5868102`/`222c810`.** Their red/green claims come from log prose only. The 52 remediation lanes have no step records; per log.md:267 each lane got a pi review.
8. **Shown only in tests or screenshots:** S33o's live refresh and keyboard scrolling of the Timeline ([S33o](../progress/pustman-v1/steps/S33o.md)); `{{var}}` resolution in the running app (S33e).
9. **Tracker and docs disagree with the code.** #199–#202 are open although S33n, S33o and `c4be074` claim the fixes. `origin/pustman-v1/main` is at `682ab32`, one commit ahead of the candidate, and that commit corrects `docs/ARCHITECTURE.md`, so the candidate's copy is stale.
10. **Not re-run by the brief writer:** the full `scripts/pustman-v1-verify.sh` and the release build at `9e1d8c3`.

---

## Appendix
**Flag reasons.**
- S27c: pasted cURL replaces the request.
- S27d: adds a persisted `RequestSettings` field.
- S28b: auth secrets (#178, now closed).
- S28c: per-request TLS-off and re-send.
- S30e: a `Set-Cookie` leak through `Debug` (MEDIUM, fixed).
- S31b: masks `Authorization`.
- S32a: the `Debug` URL leak (#108).
- S32b: redacted wire text and `refused_count`. This closes the S3 CARRY-FORWARD at `crates/console/src/feed.rs:62`.
- S32c: export re-redaction.
- S33c: keychain reads.
- S33e: auth, secrets and history now live.
- S33w: history writes.
- S33o: history-row ordering.
- S33p: the cookie jar is persisted.
- S34a: kv write.
- S34b: secret reveal.
- S34w: makes S34a live.
- S35c: delete/clear.
- S35w: wires the deletes.
- S36a: cookie delete.
- S36b: `save_jar`.
- S37: redaction NFR.

**Remediation lanes touching risky areas** (not in the rate): `90ec5a4` sd (console feed and `client.rs`, the secret-URL redaction fix cycle). Storage: `04fb616`, `f9d5353`. History: `d862e93`, `6e0f900`. Deletes: `2521efb`. Cookies: `10939d3`, `b9127b2`, `aefa93f`. cURL: `c4a2da1`, `e11b3ae`.

**Other non-step commits.** `5868102`, `c31e3b5`, `774b97c`, `7688dc2`, `dabaedc`, `222c810`, `0a87f87`, `8a9b913`, `a5fcbbb`, `40deef7` (merge).

**Tests.** 194 test files were added. One pre-existing test file was modified (`reqwest_transport_tests.rs`), and `affc474` changed one test file that was born in the window. No test file was deleted.

**Open `medium` issues:** #31, #199, #200, #201, #202.
