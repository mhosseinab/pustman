# Pustman v1 (M0–M5) — Step-by-Step Implementation

**Status: READY TO EXECUTE (2026-11-17). Revised 2026-11-19 into smaller steps** (see
[Smaller steps](#smaller-steps-2026-11-19-revision)). **Expanded 2026-11-21 with M3–M5** (S49–S71, from
[Phase 6](#phase-6--protocol-foundation-shared-by-m3-m4-and-m5); STUB-M6/M7 remain). Companion to [`pustman-v1-plan.md`](pustman-v1-plan.md)
(the *why*). This is the *how*: ordered, **standalone, independently verifiable** steps, each with a
prompt you can paste as-is and an explicit Verify block. **Read-only for workers**: a worker never
edits a Goal, a Verify block, an existing test, or this file.

## How to use this

The driver ([`pustman-v1-orchestrator-prompt.md`](pustman-v1-orchestrator-prompt.md)) runs each step this way:

1. It wraps the fenced **prompt** in the worker contract and gives it to a fresh worker. The contract covers the session-start ritual, the Edits set, no test tampering, no stubs, running Verify and pasting its output, committing `S<N>: <title>`, and stopping.
2. A fresh reviewer re-runs the **Verify** block.
3. The integrator squash-merges the step onto `pustman-v1/main` and writes the step record.

Steps run in graph order, and steps with disjoint Edits and no edge between them run in parallel.
Because of that contract, prompts below describe only the change itself and the rules it touches.

- **Nothing is flagged.** The repo has no flag mechanism and is greenfield, so every step is
  **additive**. Nothing destructive exists in M0–M2.
- **Human gates:**
  - **G-DEPS**, before any `[GPUI]` step
  - **S25**, the M0 sign-off
  - **S38**, the M1 sign-off
  - **S48**, the M2 sign-off
  - **S60**, **S67** and **S71**, the M3, M4 and M5 sign-offs
  - **STUB-M6** and **STUB-M7**, the expansion gates
- **`[GPUI]`** marks a step whose crate depends on `gpui`. Its Verify cannot run until G-DEPS clears
  (see plan §2).

## Decisions baked in (final, 2026-11-17, from plan §4)

- **D1.** Workspace files are **JSON**:
  - pretty-printed, 2-space indent, with a stable struct field order
  - maps are `BTreeMap` or sorted `IndexMap`
  - trailing `\n`
  - written atomically: temp file in the same directory, fsync, then rename
- **D2.** Timing phases: S12 spikes what `reqwest` exposes. Unmeasurable phases are **omitted, never
  estimated**. A custom hyper connector is allowed **only inside** `ReqwestTransport`. ADR-0003 records the outcome.
- **D3, D4, D5:** Socket.IO is v4 only (M4). There is no scripting runtime; imported scripts are preserved verbatim and never run. Sync is deferred.
- **D6.** SQLite is accessed via **`rusqlite`** with the `bundled` feature. Its API is synchronous and is called through `cx.background_spawn`.
- **D7.** `api_model::Request` is `#[serde(tag = "type")]`. M0–M2 have only `Http`. Unknown types
  round-trip as `Unsupported(serde_json::Value)`, and later milestones add variants additively.
- **D8.** There is one `api_model::redact` module (pure, no IO). History, Console and every export use it.
- **D9.** Test placement: `#[cfg(test)] #[path = "<file>_tests.rs"] mod tests;` and integration tests in
  `crates/<c>/tests/`. **No inline `mod tests {` in `src/*.rs`**; the tripwire counts it.
- **D10.** `pustman_actions` declares every PRD action, and `assets/keymaps/*.json` binds them all, in
  S21. After S21, feature steps implement handlers and never edit keymaps.
- **D11.** Promotion is a tag `release/pustman-v1-<n>` at four points:

  | Tag | Taken at | Covers |
  | --- | --- | --- |
  | `-1` | G-DEPS | S0–S10, domain only |
  | `-2` | M0 | S11–S25 |
  | `-3` | M1 | S26–S38 |
  | `-4` | M2 | S39–S48 |

  The window cap is unapplied deltas ≥ 3 or unpromoted steps ≥ 16.

## Project rules every prompt must respect (stated once)

**Source:** `AGENTS.md`, `.claude/rules/*.md`. Prompts restate only the rules they touch.

**Correctness**
- No `unwrap()` or `expect()` in non-test code. Clippy denies them via `[workspace.lints]`.
- No panicking indexing on uncontrolled data, and no `let _ = fallible()`.
- Errors reach the user: as a toast or an inline error, and never only logged.

**Crate structure**
- Domain crates (`api_model`, `variables`, `workspace_store`, `storage`, `secrets`, `git`) never
  depend on `gpui` or on a UI crate.
- Library roots are `src/<crate_name>.rs` via `[lib] path`. There is no `mod.rs`.
- Every crate has `[lints] workspace = true` and `pub fn init(cx: &mut App)` (GPUI crates only).
- All dependency versions live in the root `[workspace.dependencies]`. Crates use `x.workspace = true`.
- `Cargo.lock` is **regenerated at merge**; workers do not resolve lock conflicts.

**GPUI**
- `render` is pure: no `cx.notify()`, no `update`.
- A dropped `Task` cancels its work; store `Task` and `Subscription` handles.
- No blocking IO on the foreground; use `cx.background_spawn`/`gpui_tokio`.
- Use `SharedString::from_static` for literals.
- Element IDs come from stable model IDs.
- Every interactive element is keyboard-reachable and labelled.

**UI work**
- Load the `rust-gpui-ui-ux` skill first.
- Check `.claude/skills/rust-gpui-ui-ux/references/ux-checklist.md` and report which items you
  verified and which you could not.

**Async and processes**
- Use `smol::process` (never `std::process::Command`) and `serde_json::from_slice` (never `from_reader`).
- In tests, use GPUI timers only.

**Security**
- Never log bodies, `Authorization`, cookies, or secret values.
- Secrets live only in the OS keychain.
- TLS verification is on by default.
- Imports are untrusted: 50 MB limit, depth limit, no script execution.

**Licensing:** Zed's `ui`/`theme`/`workspace`/`settings`/`picker`/`editor`/`db` are GPL. Study the pattern, write
our own. Never paste their code.

**Docs and third-party libraries**
- Docs ship with the code they describe. Update `docs/ARCHITECTURE.md` when you add a crate.
- Third-party APIs: check current docs with Context7 before using a library, and pin versions in the root manifest.

**Read-only material:** existing tests (`**/tests/**/*.rs`, `**/*_tests.rs`, `**/test_support.rs`), this
doc and every Verify block. A later step that needs more test helpers adds a NEW file; it never edits an
earlier step's helper. No stubs, `todo!()`, `unimplemented!()` or TODO comments.

## Commands (the only ones Verify blocks use)

| Purpose | Command | Expected |
|---|---|---|
| Bootstrap a worktree | `scripts/pustman-v1-init.sh` (S0) | exit 0; prints `TOOLCHAIN=ok` and `GPUI_DEPS=ok\|missing` |
| Require GPUI deps | `scripts/pustman-v1-init.sh --require-gpui` | exit 0 only when every GPUI system lib is present |
| Fast verify (loop) | `scripts/pustman-v1-test.sh <crate> [filter]` (S0) | `PASS <n>` with n > 0, else `ERROR …` and exit 1 |
| Full verify (merge gate) | `scripts/pustman-v1-verify.sh` (S0) | last line `VERIFY OK` (fmt check, clippy `-D warnings`, `cargo test --workspace`, tripwire) |
| Tripwire | `scripts/pustman-v1-tripwire.sh` (S0) | prints `TRIPWIRE <n>`; exits 1 if n > baseline `0` |
| E2E (UI steps) | `cargo run -p pustman` + the scenario in the step | the observed behavior (needs a display; otherwise it goes to the deferred gate) |
| Test files untouched | `git diff --name-status pustman-v1/main...HEAD -- 'crates/**/tests/**' 'crates/**/*_tests.rs' 'crates/**/test_support.rs' \| grep -v '^A'` | empty |


## Smaller steps (2026-11-19 revision)

The original 49 steps were sized for one long worker session each. This revision splits every large
step into **sub-steps** `S<N>a`, `S<N>b`, … that each fit a short session in any harness (Claude
Code, pi, opencode), with one module, two to four tests and a module-scoped Verify. Steps that were
already passed or small stay whole, and their text is unchanged except for dependency IDs. The
numbering keeps D11: a sub-step belongs to its parent's milestone tag (`S26a` is in `-3`).

Five conventions make the sub-steps independent:

1. **One new module per test-adding sub-step.** Existing test files are read-only, so a sub-step
   that adds tests creates `src/<module>.rs` + `src/<module>_tests.rs` (wired with
   `#[cfg(test)] #[path = "<module>_tests.rs"] mod tests;`). Its Verify filters on `<module>::`, so
   the expected count is exact and no cumulative `n ≥` counts remain. Module names are unique inside
   a crate, and no module name is a substring of another one in the same crate's filters.
2. **Append-only shared lines are not an overlap.** Two unordered sub-steps may both append to:
   - the crate root's `mod` declarations,
   - one-line `<module>::init(cx)` calls inside the crate's `pub fn init`,
   - `[dependencies]` lines in the crate's `Cargo.toml`,
   - rows in `docs/ARCHITECTURE.md`.

   They may not both edit any other line of the same file. The integrator resolves conflicts in
   these appended lines by keeping both sides; anything else is a rebase for the rebase-resolver.
3. **Module-local `impl` blocks.** A sub-step that adds behavior to an entity defined elsewhere adds
   an `impl <Entity>` block in its own module instead of editing the entity's file, when the entity's
   fields already allow it. When a new field is needed, the entity's file is in the sub-step's Edits
   and the graph orders it after every other sub-step that edits that file.
4. **Wire steps own the binary.** S22e turns `crates/pustman` into a library (`src/pustman.rs` with
   `pub fn init(cx: &mut App)` holding the init order) plus a thin `src/main.rs` that only opens the
   window. Afterwards, a crate gets registered in the app only by a **wire step** `S<N>w`. A wire step:
   - edits `crates/pustman/src/pustman.rs` and `crates/pustman/Cargo.toml`,
   - adds `crates/pustman/tests/wire_<feature>.rs` whose test functions are named `wire_<feature>_…`,
   - verifies with `scripts/pustman-v1-test.sh pustman wire_<feature>` → `PASS 1` and
     `cargo build -p pustman` → exit 0, plus the step's E2E scenario when it has one.

   Feature sub-steps never touch `crates/pustman/**`. The M3–M5 wire steps (S59w, S65w, S69w) leave
   `pustman.rs` alone, because their feature crates are already registered: they append dev-deps to
   `crates/pustman/Cargo.toml` and add the `wire_<feature>.rs` test.
5. **Serial files stay serial.** A crate's main view file (`request_editor.rs`, `response_viewer.rs`,
   `tree.rs`, `workspace.rs`, `reqwest_transport.rs`) is edited by one chain of sub-steps at a time;
   the graph below orders every pair that shares one. Parallelism comes from new modules and from
   different crates.

**High-stakes sub-steps** (consensus review, as before): S13c and S16a (secrets), S28c (TLS),
S43a/S43b/S43c (secrets), S45a/S45b (import), plus the unsplit S12, S37 and S47. M3 adds S50c and
S52a (import/auth model), S55a–S55d and S56a–S56d (auth and OAuth secrets), S57a–S57c (proxy,
certificates, TLS) and S58b (OAuth form).

**In-flight mapping** (2026-11-19): S12 is unchanged. The S19 worktree already holds most of the
component files; its work continues as **S19a**, and S19b/S19c start after S19a merges.

## Step counts (2026-11-19 revision)

Measured by a validator script over this file and the 2026-11-17 version. Stubs and G-DEPS are not counted.

| Measure | 2026-11-17 | 2026-11-19 | 2026-11-21 (M3–M5 expanded) |
| --- | --- | --- | --- |
| Steps (human sign-offs included) | 49 | 109 | 167 |
| Steps touching a hotspot | 9 (18%) | 9 (8%) — only the wire steps | 12 (7%) — only the wire steps |
| Longest dependency chain, in steps | 23 | 31 | 45 |
| Widest graph level (steps with no edge between them) | 5 | 10 | 10 |
| Unordered step pairs sharing a non-append, non-hotspot file | — | 0 | 0 |
| Original test names missing from this file | — | 0 | 0 |

For the 2026-11-21 column, every new step's `PASS n` was also checked against the test names its prompt
lists. The longest chain now runs on past S48 through
S49→S51a→S51b→S51c→S51d→S54c→S59b→S69a→S69b→S69c→S69d→S69w→S70→S71.

The chain is longer in steps but shorter in work, because each step on it is smaller. The longest chain runs
S0→S1→S17→S18→S19a→S22a→S22b→S23a→S23b→S25→S26a→…→S28c→S29e→S33a→S33b→S33c→S38→S39a→S39b→S40a→S40b→S42a→S45a→S45b→S48.
The driver's concurrency cap (3 workers) limits how much of the wider graph runs at once. Raising the cap
is a driver setting; this file does not change it.


## Hotspots (the driver serializes them even when the graph says independent)

```
hotspots:
  - Cargo.lock                      # regenerate at merge, never hand-merged
  - crates/pustman/src/pustman.rs   # init order (from S22e); touched only by wire steps
  - crates/pustman/src/main.rs      # thin window opener; touched only by S22e
  - crates/pustman/Cargo.toml       # same steps as pustman.rs
```

Only the wire steps touch these files: S22e, S24w, S26w, S29w, S32w, S34w, S35w, S39w and S44w, and
for M3–M5 S59w, S65w and S69w (`crates/pustman/Cargo.toml` dev-deps only). The
validation numbers are in [Step counts](#step-counts-2026-11-19-revision). Two earlier design
choices still keep other files off this list:

- The root `Cargo.toml` uses `members = ["crates/*"]` and declares every dependency in S1 (M0–M2)
  and S49 (M3–M5).
- Keymaps are written once in S21.

## Dependency graph (quick view)

`A→B` = B depends on A. Steps on separate lines with no edge between them run in parallel.
`⟂G` = waits for human gate G-DEPS. Each step's own **Depends on** line is authoritative; this view
abbreviates.

```
Phase 0 harness        S0
Phase 1 domain         S0→S1 ; S1→S2→S3→S9→S10 ; S2→S4 ; S2→S5→S8 ; S1→S6 ; S1→S7
                       ── G-DEPS (human: install GPUI libs, push for CI) ── promote release/pustman-v1-1 ──
Phase 2 http_engine ⟂G S2,S4→S11→S12
                       S12→S13a ; S12→S13b ; S12→S14b ; S12→S15a          (parallel, new modules; S12 declares http_engine/**)
                       S13a,S13b,S12,S7→S13c ; S12→S14c
                       S13c,S14b,S14c→S14a ; S15a,S6→S15b ; S15a,S14a→S15c
                       S13c,S14c,S3,S6→S16a→S16b
Phase 3 shell ⟂G       S1→S17→S18→S19a→{S19b, S19c} ; S1→S21
                       S19a→S20a→S20b→S20c
                       S19a,S21→S22a→{S22b, S22c, S22d}
                       S22a,S22d,S6,S17,S18,S21→S22e (wire)
                       S22b,S22c,S22d,S6→S23a→S23b
                       S19a,S20a→S24a ; S24a,S21,S22d→S24b ; S24b,S22e→S24w
                       S19b,S19c,S20c,S23b,S24w→S25 (M0 SIGN-OFF, human) ── promote -2
Phase 4 HTTP UI        request_editor  S25,S15c,S16b→S26a→S26b→S27a→S27b→S27c→S28a→S28b→S28c
                                       S26b,S22e→S26w
                       response_viewer S25,S16b→S29a→{S29b, S29c}
                                       S29b→S30a→S30b→S30c
                                       S29c→S30d→S30e→S31a→S31b ; S30e,S15b,S22d→S36a→S36b
                                       S29c,S28c→S29e ; S29c,S26w→S29w
                       variables UI    S29e→S33a→S33b→S33c ; S29e→S33d
                       console         S25,S3,S11→S32a→{S32b, S32c, S32d} ; S32b,S32c,S32d,S22e→S32w
                       environments    S25,S4,S7,S6→S34a→S34b ; S34b,S22e→S34w
                       history         S25,S16b,S22c,S6→S35a→{S35b, S35c} ; S35b,S35c,S22e→S35w
                       security        S5,S16b,S15c,S32c→S37
                       Phase 4 leaves→S38 (M1 SIGN-OFF, human) ── promote -3
Phase 5 workspaces     S38→S39a→S39b→{S39w, S40a} ; S40a→S40b ; S38→S40c
                       S38,S23b,S39a→S41a→S41c ; S41a,S24b→S41b
                       S40b,S41a,S8→S42a ; S40b→S42b ; S40c,S42a→S42c
                       S38→S43a→S43b→S43c
                       S38→S44a ; S44a,S41a→S44b→S44w
                       S9,S42a,S40a→S45a→S45b ; S40b→S45c
                       S41a,S24a→S46 ; S37,S43b,S44a→S47
                       Phase 5 leaves→S48 (M2 SIGN-OFF, human) ── promote -4
Phase 6 foundation    S48→S49 ; S49→S50a→{S50b, S50c} ; S49→S50d ; S50a,S50b→S50e
                       S49→S51a→S51b→S51c→S51d ; S51a→S51e
Phase 7 M3             S50c→S52a ; S49→S52b ; S50a→S53 ; S51a→S54a ; S54a,S53→S54b ; S54b,S51d→S54c
                       auth     S52a,S54b→S55a ; S52a→S55b ; S52a→S55c ; S55a,S55b,S55c→S55d
                       oauth    S55d→S56a→S56b→S56c→S56d
                       network  S52b,S54b→S57a→S57b ; S57b,S50d→S57c
                       ui       S52a,S57c→S58a ; S58a,S56c→S58b ; S53,S50d,S50e,S58b→S59a ; S54c→S59b
                       S59a,S59b,S56d→S59w (wire) ; S54c,S57c,S58b,S59w→S60 (M3 SIGN-OFF, human) ── promote next tag
Phase 8 M4             S50a,S51a,S52b→S61a→{S61b, S61c} ; S61a→S62a→S62b
                       S61b,S61c,S51d,S50d,S50e→S63a→S63b ; S62b,S63a→S64
                       S51e,S63a,S63b,S64→S65 ; S65,S63b→S65w (wire) ; S65w→S66→S67 (M4 SIGN-OFF, human) ── promote next tag
Phase 9 M5             S50a→S68a→{S68b, S68c} ; S68b,S68c,S52b→S68d ; S68d,S51a→S68e
                       S59b,S68d→S69a ; S68d,S69a,S50d,S50e→S69b ; S69b,S68e,S51d→S69c ; S69b,S69c→S69d
                       S69c,S69d→S69w (wire) ; S69w→S70→S71 (M5 SIGN-OFF, human) ── promote next tag
Phase 10+ (stubs)      S51d,S50e→STUB-M6 ; S60,S67,S71,STUB-M6→STUB-M7
```

These chains are serial on purpose, because each shares a file:
- S3→S9→S10 share `crates/api_model/src/api_model.rs`.
- `reqwest_transport.rs`: S14c→S14a→S15c. `client.rs`: S13c→S16a.
- `request_editor.rs`: S26a→…→S28c→S29e→S33d, and S33a–c edit the field modules after S29e.
- `response_viewer.rs`: S29a→S29c→S30d→S30e→S31a; `body_view.rs`: S29b→S30a→S30b→S30c.
- `tree.rs`: S39a→S39b→S40a→S40b→S42b.
- `workspace.rs`: S22a→S22d. S22b and S22c each add their own module.
- M3–M5: `postman.rs`: S50c→S52a. `client.rs`: S53→S54b→S55a→S56c. `reqwest_transport.rs`:
  S54b→S57a→S57b. `auth_tab.rs`: S58a→S58b. `response_viewer.rs`: S54c→S59b→S69a. `session_log.rs`:
  S51c→S51d. `websocket_editor.rs`: S63a→S63b→S65. `socketio_editor.rs`: S64→S65. `grpc_editor.rs`:
  S69b→S69c→S69d. `pustman_actions.rs`: S50d→S57c. `crates/pustman/Cargo.toml` (hotspot): S59w, S65w,
  S69w, serialized by the driver. `api_model/src/request.rs` is edited only by S50a.

---

# Phase 0 — Harness

### S0 — Harness: bootstrap, test runner, tripwire, hooks, promotion tooling, subagents
**Goal:** after this step:
- A fresh worktree is runnable with one command that reports whether GPUI deps are present.
- `pustman-v1-test.sh` refuses zero-test passes.
- The no-unwrap invariant has a tripwire.
- The steps doc and existing tests are hook-protected.
- Promotion is script-gated.
- The loop's subagents live in the repo.
- `pustman-v1/main` exists.

**Depends on:** none.
**Deploy delta:**
- `manual(install GPUI system packages on every dev/build host: libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev — this is gate G-DEPS)`
- `codified(.claude/settings.json hooks)`

**Edits:**
- **Scripts:** `scripts/pustman-v1-init.sh`, `scripts/pustman-v1-test.sh`, `scripts/pustman-v1-verify.sh`, `scripts/pustman-v1-tripwire.sh`, `scripts/promote-check.sh`
- **Claude config:** `.claude/hooks/protect-steps-and-tests.sh`, `.claude/settings.json`, `.claude/agents/*.md` (new files only), `.claude/skills/explain/**`, `.claude/skills/promote/**`, `.claude/skills/pustman-settings-persistence/SKILL.md`
- **Docs:** `DEPLOY_DELTAS.md`, `docs/ARCHITECTURE.md`, `docs/decisions/ADR-0000-template.md`, `docs/decisions/ADR-0001-workspace-format-json.md`, `docs/decisions/ADR-0002-sqlite-rusqlite.md`, `docs/briefs/.gitkeep`, `AGENTS.md`
- **Other:** `.gitignore`

```
Read AGENTS.md, CLAUDE.md, .claude/rules/tests.md, docs/pustman-v1-plan.md (§2, §4, §8–§10), and the
implementation-orchestration skill's scaffold/ and agents/ directories at
~/.claude/skills/implementation-orchestration/{scaffold,agents}/ (read agents/README.md first).
This step builds tooling only — no Rust crates, no application code.

1. scripts/pustman-v1-init.sh (bash, `set -euo pipefail`, idempotent): checks `rustc --version`
   matches 1.98.1 (print `TOOLCHAIN=ok` or one `ERROR toolchain …` line + exit 1); probes
   `pkg-config --exists` for xkbcommon xkbcommon-x11 wayland-client x11-xcb vulkan fontconfig freetype2
   and prints `GPUI_DEPS=ok` or `GPUI_DEPS=missing <libs>` (exit 0 either way). With
   `--require-gpui`: missing libs → one `ERROR GPUI system libs missing: <libs>` line and exit 1.
   On macOS skip the pkg-config probe and print `GPUI_DEPS=ok`.
2. scripts/pustman-v1-test.sh <crate> [filter]: runs `cargo test -p <crate> -- <filter>` quietly,
   sums every `test result: ok. N passed` line; on any failure or non-zero exit prints
   `ERROR <first failing test or error line>` and exits 1; if the sum is 0 prints
   `ERROR 0 tests ran` and exits 1; else prints exactly `PASS <n>`.
3. scripts/pustman-v1-tripwire.sh: counts, across `crates/*/src/**/*.rs` EXCLUDING files named
   `*_tests.rs` and `test_support.rs` and anything under `tests/`, the occurrences of `.unwrap()`,
   `.expect(`, `std::process::Command`, `serde_json::from_reader`, `let _ =`, `smol::Timer`,
   `std::thread::sleep`, and `mod tests {`. Prints `TRIPWIRE <n>`; exits 1 if n > 0 (baseline 0).
   With no crates yet it prints `TRIPWIRE 0`.
4. scripts/pustman-v1-verify.sh: runs `cargo fmt --all -- --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo test --workspace`, the tripwire; prints one line per stage
   (`fmt ok` …) and `VERIFY OK` last, or `VERIFY FAIL <stage>` and exit 1. If there is no
   Cargo.toml yet, print `VERIFY OK (no workspace yet)` after the tripwire.
5. Protection hook: .claude/hooks/protect-steps-and-tests.sh — a PreToolUse hook for
   Edit|Write|NotebookEdit reading the tool JSON on stdin (jq), blocking (exit 2 + a stderr reason)
   when file_path matches *pustman-v1-implementation-steps.md, or matches */tests/*.rs, *_tests.rs or
   *test_support.rs AND already exists on pustman-v1/main (`git cat-file -e pustman-v1/main:<relpath>`)
   — new test files stay writable. Register it in .claude/settings.json, plus a SessionStart hook
   (matcher `compact`) that echoes "Read docs/pustman-v1-progress.md and tail -n 30
   docs/progress/pustman-v1/log.md before acting."
6. Promotion tooling from the scaffold: scripts/promote-check.sh with SLUG=pustman-v1,
   FULL_VERIFY="scripts/pustman-v1-verify.sh", ENV_VALIDATE="scripts/pustman-v1-init.sh
   --require-gpui", TRIPWIRE_CMD="scripts/pustman-v1-tripwire.sh | awk '{print \$2}'",
   TRIPWIRE_BASELINE=0, TEST_PATHS="crates/*/tests crates/*/src/*_tests.rs crates/*/src/**/*_tests.rs
   crates/*/src/test_support.rs", MIGRATIONS_DIR="crates/storage/migrations",
   LEDGER=DEPLOY_DELTAS.md, MAX_MANUAL=2. There is no staging: document at the top of the script
   that "staging" for this desktop app means the build host. DEPLOY_DELTAS.md, .claude/skills/explain,
   .claude/skills/promote (adapt the promote skill: tag → `cargo build --release -p pustman` → human
   manual smoke on Linux + macOS; no deploy), docs/briefs/.gitkeep,
   docs/decisions/ADR-0000-template.md, docs/ARCHITECTURE.md filled from plan §3/§5 and marked
   "target — crates appear as steps land".
7. ADRs: ADR-0001 (D1 JSON workspace format) and ADR-0002 (D6 rusqlite) from plan §4, status Accepted.
8. Subagents: copy ~/.claude/skills/implementation-orchestration/agents/*.md into .claude/agents/,
   EXCEPT mobile-implementer, platform-reviewer and ui-conformance-reviewer (web-oriented; the
   repo's gpui-ui-reviewer and ux-auditor cover UI). Never overwrite the existing repo agents
   (gpui-test-writer, gpui-ui-reviewer, http-engine-engineer, rust-architect, ux-auditor). Replace
   every <placeholder> with this repo's values: rules doc AGENTS.md, bootstrap/test/verify/tripwire
   commands above, hotspots from docs/pustman-v1-implementation-steps.md, test globs from
   .claude/rules/tests.md, the gh label `pustman-v1,low`.
9. AGENTS.md: replace the "pre-scaffold" status note with the PRD §9 crate list (add workspace_store,
   secrets, console, git, pustman_actions, integration_tests; later milestones: websocket_client,
   grpc_client, mqtt_client); add sections: hotspots, squash-merge `S<N>: <title>` onto
   pustman-v1/main, tags `release/pustman-v1-<n>`, test placement rule D9 (no inline `mod tests {`),
   "docs ship only with the code they describe".
10. .claude/skills/pustman-settings-persistence/SKILL.md: state that Postman v2.1 is the import/export
    format only and the on-disk format is the PRD §6.1 folder of JSON files (link docs/PRD.md §6.1).
11. .gitignore: add `/target`, `.claude/worktrees/`.
12. Create the integration branch: `git branch pustman-v1/main` from the current HEAD (no push).
Do not create any crate. Do not edit docs/PRD.md or any pustman-v1-* doc.
```

**Verify:**
- `scripts/pustman-v1-init.sh; echo "exit=$?"` → `TOOLCHAIN=ok`, a `GPUI_DEPS=` line, `exit=0`.
- `scripts/pustman-v1-init.sh --require-gpui; echo "exit=$?"` behaves differently by host:
  - **This host:** one `ERROR GPUI system libs missing: …` line and `exit=1`.
  - **Host with the libs installed:** `exit=0`.
- `scripts/pustman-v1-test.sh nosuchcrate; echo "exit=$?"` → an `ERROR …` line, `exit=1`.
- `scripts/pustman-v1-tripwire.sh` → `TRIPWIRE 0`.
- `scripts/pustman-v1-verify.sh | tail -1` → `VERIFY OK (no workspace yet)`.
- Hook blocks the steps doc:
  `echo '{"tool_name":"Edit","tool_input":{"file_path":"docs/pustman-v1-implementation-steps.md"}}' | .claude/hooks/protect-steps-and-tests.sh; echo "exit=$?"` → `exit=2`.
- Hook allows a new test file:
  `echo '{"tool_name":"Write","tool_input":{"file_path":"crates/x/src/new_tests.rs"}}' | .claude/hooks/protect-steps-and-tests.sh; echo "exit=$?"` → `exit=0`.
- `bash -n scripts/*.sh .claude/hooks/*.sh` → no output.
- `scripts/promote-check.sh HEAD` → prints one PASS/FAIL line per item. The only FAILs it should print are for items where no tags or deltas exist yet, or for the env validator on this host.
- `ls .claude/agents/*.md | wc -l` → ≥ 25.
- `grep -l '<placeholder>\|<slug>' .claude/agents/*.md | wc -l` → `0`.
- `git branch --list pustman-v1/main` → present.
- `grep -c 'pre-scaffold' AGENTS.md` → `0`.

---

# Phase 1 — Domain crates (GPUI-free; verifiable now)

### S1 — Cargo workspace, toolchain, lints, CI, `api_model` skeleton
**Goal:** `cargo test --workspace` runs a real test on Linux and macOS in CI. Every dependency the
milestones need is declared once in the root manifest.
**Depends on:** S0.
**Deploy delta:** `codified(.github/workflows/ci.yml — CI on push/PR, Linux + macOS)`.
**Edits:** `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `.github/workflows/ci.yml`,
`crates/api_model/**`.

```
Read AGENTS.md (Toolchain, Commands, Rules), .claude/rules/cargo-manifests.md, .claude/rules/tests.md,
the pustman-feature-crate skill, and Zed's root Cargo.toml for the [workspace.lints] shape (patterns
only). Use Context7 to confirm current crate versions.
1. rust-toolchain.toml: channel "1.98.1", components rustfmt + clippy. rustfmt.toml: style_edition
   = "2024". clippy.toml: allow-unwrap-in-tests = true, allow-expect-in-tests = true.
2. Root Cargo.toml: `[workspace] resolver = "3"`, `members = ["crates/*"]`, `[workspace.package]
   edition = "2024"`, `publish = false`; `[workspace.lints.clippy]` unwrap_used = "deny",
   expect_used = "deny", dbg_macro = "deny", todo = "deny", unimplemented = "deny";
   `[workspace.lints.rust]` unsafe_code = "deny".
   `[workspace.dependencies]`: declare NOW every dependency M0–M2 need so later steps never edit
   this file: the three GPUI git deps exactly as in AGENTS.md (rev
   58849026ec30fb225dc5d07cccf37e8408b85555); serde (derive), serde_json (preserve_order), anyhow,
   thiserror, log, ulid (serde), url (serde), indexmap (serde), smol, futures, async-trait, reqwest
   (default-features=false, rustls-tls, cookies, stream, gzip, brotli, deflate, multipart, json),
   rusqlite (bundled), keyring, notify, tempfile, shlex, base64, percent-encoding, rand, time
   (formatting, serde), uuid (v4), regex, mime, mime_guess, cookie_store, hex, similar; dev/test:
   pretty_assertions, tokio (rt-multi-thread, macros, net), hyper (server, http1), hyper-util,
   http-body-util, bytes, rcgen, tokio-rustls, rustls. Plus each crate in this workspace by path
   (api_model, variables, workspace_store, storage, secrets, http_engine, settings, theme, ui,
   text_input, pustman_actions, workspace, command_palette, request_editor, response_viewer,
   console, environment_picker, history_panel, collections_panel, git) — path entries for crates
   that do not exist yet are fine only if cargo accepts them; if cargo rejects a missing path,
   omit those and note it as a Decision (each later step then adds its own path line — tell the
   driver this makes the root Cargo.toml a hotspot).
3. crates/api_model: Cargo.toml ([lib] path = "src/api_model.rs", [lints] workspace = true, deps
   serde, serde_json, ulid), src/api_model.rs defining `ItemId(ulid::Ulid)` with `ItemId::new()`,
   Display/FromStr, serde as a string; tests in src/api_model_tests.rs via
   `#[cfg(test)] #[path = "api_model_tests.rs"] mod tests;`: item_id_roundtrips_through_json,
   item_id_rejects_invalid_string.
4. .github/workflows/ci.yml: jobs on ubuntu-latest and macos-latest; ubuntu installs
   libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev
   libfontconfig-dev libfreetype-dev libdbus-1-dev; steps: checkout, toolchain from
   rust-toolchain.toml, cache, `scripts/pustman-v1-verify.sh`.
No other crates. No unwrap/expect outside tests.
```

**Verify:**
- `scripts/pustman-v1-test.sh api_model item_id` → `PASS 2`.
- `scripts/pustman-v1-verify.sh | tail -1` → `VERIFY OK`.
- `grep -c 'rev = "58849026ec30fb225dc5d07cccf37e8408b85555"' Cargo.toml` → `3`.
- The workflow file parses: `python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/ci.yml'))"` exits 0.

### S2 — `api_model` types: collections, folders, HTTP requests, auth, variables, settings, examples
**Goal:** the PRD §4 model for M0–M2 round-trips through serde. Unknown fields and types survive
verbatim, and auth inheritance resolves.
**Depends on:** S1. **Deploy delta:** none.
**Edits:** `crates/api_model/**`.

```
Read docs/PRD.md §4 (REQ-MODEL-01..04), §5.1 REQ-HTTP-01/04/06/09, §6.1–6.2, the pustman-http-engine
skill (model section), and crates/api_model/src/api_model.rs. Add modules (one file each, tests in
sibling *_tests.rs): collection.rs (Workspace{id,name,format_version}, Collection, Folder with
`order: Vec<ItemId>`), request.rs (`#[serde(tag = "type")] enum Request { Http(HttpRequest),
#[serde(other)]-style fallback Unsupported(serde_json::Value) }` — implement the fallback with a
custom Deserialize so the raw value is kept; HttpRequest{id,name,method: Method (standard verbs +
validated custom token), url, params, path_variables, headers, body: Body (None|FormData|
UrlEncoded|Raw{language}|Binary{file}|Unsupported), auth, settings, examples}), auth.rs (Auth =
Inherit|None|Basic|Bearer|ApiKey{location: Header|Query}|Unsupported(Value); `effective_auth(chain:
&[&Auth]) -> (EffectiveAuth, AuthSource)` walking request → folder → collection), variable.rs
(Variable{key, value: Option<String>, kind: Default|Secret, enabled, description}; a Secret
variable never serializes a value — enforce in Serialize), environment.rs, settings.rs
(RequestSettings: every field Option = "inherit global": timeout_ms, follow_redirects,
max_redirects, follow_original_method, verify_tls, encode_url, remove_referer, http_version),
example.rs (Example{name, request, response}).
Every struct carries `#[serde(flatten)] extra: BTreeMap<String, serde_json::Value>` so unknown
fields round-trip (REQ-MODEL-02). No IO, no gpui. Full-word names. No unwrap/expect outside tests.
Tests (names exact): request_roundtrip_preserves_unknown_fields, unknown_request_type_roundtrips,
custom_method_token_validated, secret_variable_never_serializes_value,
auth_inherits_from_folder_then_collection, auth_none_stops_inheritance,
settings_none_means_inherit.
```

**Verify:**
- `scripts/pustman-v1-test.sh api_model` → `PASS n` (n ≥ 9).
- `cargo tree -p api_model | grep -c gpui` → `0`.
- `scripts/pustman-v1-tripwire.sh` → `TRIPWIRE 0`.
- At merge: `scripts/pustman-v1-verify.sh | tail -1` → `VERIFY OK`.

### S3 — Shared `redact()` (D8)
**Goal:** a single pure function masks secrets in text, headers and URLs. History, Console and export
all depend on it.
**Depends on:** S2. **Deploy delta:** none.
**Edits:** `crates/api_model/src/api_model.rs`, `crates/api_model/src/redact.rs`, `crates/api_model/src/redact_tests.rs`.

```
Read docs/PRD.md REQ-NFR-04, REQ-HIST-03, REQ-CON-05, REQ-NET-04 and AGENTS.md › Security. Add
api_model::redact: `SecretValues` (a set of literal secret strings, ignoring empty and < 4-char
values), `redact_text(&str, &SecretValues) -> String` (replaces every occurrence with `••••••`),
`redact_headers(&[(String,String)], &SecretValues) -> Vec<(String,String)>` (Authorization,
Proxy-Authorization, Cookie, Set-Cookie values always masked, plus SecretValues anywhere),
`redact_url(&str, &SecretValues) -> String` (userinfo password and secret values in query).
Pure: no IO, no logging. Tests: redacts_secret_in_text, ignores_short_and_empty_secrets,
masks_authorization_and_cookie_headers_always, redacts_url_userinfo_and_query_secret,
redaction_is_idempotent.
```

**Verify:** `scripts/pustman-v1-test.sh api_model redact` → `PASS 5`; tripwire `TRIPWIRE 0`.

### S4 — `variables` resolver
**Goal:** `{{var}}` resolution with these properties:
- narrowest-wins precedence
- dynamic variables resolved once per send
- cycle detection
- depth limit of 10
- spans for highlighting
- secrets used are tracked

**Depends on:** S2. **Deploy delta:** none.
**Edits:** `crates/variables/**`.

```
Read docs/PRD.md §7 (REQ-VAR-01..07) and the pustman-http-engine skill's variables section (the
precedence there is authoritative: request-local → environment → collection → global; a data
scope slot exists but is unused). Create crates/variables (domain crate: no gpui; deps api_model,
rand, uuid, time, thiserror). API: `ScopeStack` (ordered scopes of api_model::Variable; disabled
variables ignored; a Secret variable's value comes from a `local_values` map supplied by the
caller, never from the file), `DynamicValues` (memoizes $guid, $timestamp, $isoTimestamp,
$randomInt, $randomUUID per send — the same name resolves to the same value within one
DynamicValues), `resolve(template, &ScopeStack, &mut DynamicValues) -> Resolved { text,
unresolved: Vec<String>, secrets_used: Vec<String> }` (unresolved names stay as literal `{{name}}`
in text), `ResolveError::Cycle(Vec<String>)` naming the cycle, `ResolveError::TooDeep` at depth >
10, `spans(template) -> Vec<VariableSpan{range, name, state: Resolved|Unresolved|Secret}>` for
highlighting. Tests: narrowest_scope_wins, disabled_variable_ignored, unresolved_kept_literal_and_listed,
dynamic_value_stable_within_one_send, cycle_is_reported_with_names, depth_over_ten_fails,
secret_value_comes_from_local_values_and_is_tracked, spans_mark_secret_and_unresolved.
```

**Verify:** `scripts/pustman-v1-test.sh variables` → `PASS n` (n ≥ 8); `cargo tree -p variables | grep -c gpui` → `0`.

### S5 — `workspace_store`: on-disk format, deterministic atomic writes
**Goal:** a workspace folder (PRD §6.1) opens into a snapshot, and saving an item writes
deterministic files atomically. Saving unchanged data is byte-identical.
**Depends on:** S2. **Deploy delta:** none.
**Edits:** `crates/workspace_store/**`.

```
Read docs/PRD.md §6.1 (layout + REQ-WSP-01..04, 06), §6.2 REQ-VAR-SHARE-01/02,
docs/decisions/ADR-0001-workspace-format-json.md, and crates/api_model. Create crates/workspace_store
(domain crate: no gpui; deps api_model, serde_json, tempfile, thiserror). Layout: pustman.json
(Workspace + format_version "1.0"), .pustman/settings.json, collections/<slug>/collection.json,
folder dirs with folder.json, `<slug>.request.json`, environments/<slug>.environment.json.
`open(root) -> Result<WorkspaceSnapshot, OpenError>` (snapshot = items by ItemId + path map; a
file that fails to parse becomes `ItemProblem{path, kind: InvalidJson{line,col}|ConflictMarkers}`
in the snapshot, not a hard error — conflict markers detected by regex `^(<{7}|>{7})( |$)` per line;
a newer major format_version → `snapshot.read_only = true`). `save_item(root, &Item) ->
Result<PathBuf, SaveError>`: deterministic JSON (2-space, sorted maps, trailing \n), atomic write
(tempfile in the same dir + persist), slug from name with -2/-3 suffix on collision, rename keeps
the id, skips the write if bytes are identical. Secret variables write only {key, kind,
description}. Never write local values. Tests: open_reads_sample_workspace,
save_is_byte_identical_for_unchanged_item, save_uses_two_space_sorted_trailing_newline,
slug_collision_gets_suffix, rename_keeps_id, conflict_markers_reported_as_problem,
invalid_json_reported_with_position, newer_major_version_opens_read_only,
secret_variable_file_has_no_value. Fixtures under crates/workspace_store/tests/fixtures/.
```

**Verify:** `scripts/pustman-v1-test.sh workspace_store` → `PASS n` (n ≥ 9); `cargo tree -p workspace_store | grep -c gpui` → `0`.

### S6 — `storage`: SQLite local store with migrations
**Goal:** `LocalStore` opens or creates the DB and applies reversible migrations. It offers typed
access to every local table that M0–M2 need.
**Depends on:** S1.
**Deploy delta:** `codified(crates/storage/migrations — applied at app start; every migration has a down)`.
**Edits:** `crates/storage/**`.

```
Read docs/PRD.md REQ-WSP-06, §8.5, REQ-NFR-05, the pustman-settings-persistence skill, and
docs/decisions/ADR-0002-sqlite-rusqlite.md. Create crates/storage (domain crate: no gpui; rusqlite
bundled, thiserror, serde_json, time). Migrations as numbered pairs under crates/storage/migrations/
(`0001_init.up.sql` / `0001_init.down.sql`), embedded with include_str!, tracked in a
`schema_migrations` table. 0001 creates: kv(key,value), history(id, workspace_id, created_at,
protocol, method, target, status, duration_ms, request_json, response_summary_json,
response_body_prefix BLOB), open_tabs(workspace_id, position, item_id, pinned, draft_json),
layout(workspace_id, json), local_values(workspace_id, scope_id, key, value),
cookies(workspace_id, domain, path, name, value, attributes_json), recent_workspaces(path,
opened_at). API (sync; callers run it off the foreground): LocalStore::open(path),
open_in_memory(), migrate(), migrate_down_to(version), and small typed repos per table (insert/
list/delete; history: insert, list_by_day, search, delete, clear, prune_older_than(days)).
No secrets ever stored here (documented on the type). Tests: migrate_creates_all_tables,
migrate_is_idempotent, every_migration_has_working_down, history_prune_respects_retention,
history_search_matches_target, kv_roundtrip, open_tabs_roundtrip_preserves_order,
local_values_roundtrip.
```

**Verify:**
- `scripts/pustman-v1-test.sh storage` → `PASS n` (n ≥ 8).
- `ls crates/storage/migrations/*.up.sql | wc -l` equals `ls crates/storage/migrations/*.down.sql | wc -l`.
- `cargo tree -p storage | grep -c gpui` → `0`.

### S7 — `secrets`: OS keychain behind a trait
**Goal:** secret values are read and written only through `SecretStore`. Production uses the OS
keychain and tests use memory.
**Depends on:** S1. **Deploy delta:** none.
**Edits:** `crates/secrets/**`.

```
Read docs/PRD.md REQ-VAR-SHARE-02, REQ-IO-02, AGENTS.md › Security. Create crates/secrets (domain
crate: no gpui; keyring, thiserror). `SecretKey{workspace_id, scope_id, variable_key}` rendered as
keyring service "pustman" + account "<workspace_id>/<scope_id>/<variable_key>". `trait SecretStore:
Send + Sync { get, set, delete }` returning `Result<_, SecretError>`; `KeyringStore` (keyring
crate) and `MemorySecretStore` (tests, also exported for other crates' tests behind a
`test-support` feature). Never log values; Debug impls must not print values. Tests:
memory_store_set_get_delete, secret_key_account_format_is_stable, debug_output_hides_value; plus
`#[ignore]` live_keyring_roundtrip (run manually at sign-off: `cargo test -p secrets -- --ignored`).
```

**Verify:** `scripts/pustman-v1-test.sh secrets` → `PASS 3`; `grep -rn 'println!\|log::' crates/secrets/src | grep -ic value` → `0`.

### S8 — `workspace_store` watcher: external changes and problems
**Goal:** external edits to a workspace (git pull, branch switch) become typed change events within
1 s. A file with conflict markers or invalid JSON is reported as a problem.
**Depends on:** S5. **Deploy delta:** none.
**Edits:** `crates/workspace_store/**`.

```
Read docs/PRD.md REQ-COLLAB-01/02 and crates/workspace_store. Add watcher.rs: `watch(root) ->
Result<(WorkspaceWatcher, Receiver<FsChange>), WatchError>` using `notify`, debounced (100 ms),
ignoring writes that workspace_store itself made (track the content hash of our last write per
path) and ignoring .git/. FsChange = ItemAdded{id,path} | ItemChanged{id,path} | ItemRemoved{id,path}
| Problem(ItemProblem) | Rescan. Map paths to ids by parsing the changed file. Use
std::sync::mpsc or smol channel (no tokio). Tests in crates/workspace_store/tests/watcher.rs using
a tempdir and a bounded wait (poll the receiver with a 2 s deadline, no sleeps in production code):
external_add_is_reported_within_one_second, external_edit_is_reported, own_save_is_not_reported,
conflict_markers_reported_as_problem_event, git_dir_changes_ignored.
```

**Verify:**
- `scripts/pustman-v1-test.sh workspace_store watcher` → `PASS 5`.
- Run it 3 times: `for i in 1 2 3; do scripts/pustman-v1-test.sh workspace_store watcher; done | sort | uniq -c` → `3 PASS 5` (not flaky).

### S9 — Postman v2.1/v2.0 import, v2.1 export, environment/globals import
**Goal:** Postman files import into the model with a report. Exports are valid v2.1. Untrusted
input limits are enforced.
**Depends on:** S3. **Deploy delta:** none.
**Edits:** `crates/api_model/src/api_model.rs`, `crates/api_model/src/postman.rs`, `crates/api_model/src/postman_tests.rs`, `crates/api_model/tests/fixtures/postman/**`.

```
Read docs/PRD.md §6.4 (REQ-IO-01/02/03/06), REQ-MODEL-02, and crates/api_model. Add
api_model::postman: `import_collection(bytes: &[u8], limits: &ImportLimits) -> Result<(Collection,
ImportReport), ImportError>` for v2.1 and v2.0 (detect by schema URL); scripts (event[]) and
unsupported auth types are preserved verbatim in `extra`/`Auth::Unsupported` and listed in the
report as "preserved, not executed"; `import_environment(bytes, limits) -> (Environment,
SecretValuesToStore, ImportReport)` — Postman `secret`-typed values are returned separately for the
caller to put in the keychain and are NOT in the Environment; globals the same. `export_collection(&Collection)
-> (Vec<u8>, ExportReport)` emitting v2.1; non-HTTP request types are skipped and listed as
warnings. ImportLimits default: 50 MB, nesting depth 64; exceeded → ImportError::TooLarge /
TooDeep before full parsing (check len first; depth via serde_json with a depth-limited
deserializer or a pre-scan). Use serde_json::from_slice. Never execute anything. Fixtures:
small hand-written v2.1, v2.0, environment with a secret, a 65-deep nested file. Tests:
imports_v21_collection_with_folders_and_auth, imports_v20_collection,
scripts_preserved_and_reported_not_executed, environment_secret_values_returned_separately,
export_v21_roundtrips_core_fields, export_skips_unsupported_request_types_with_warning,
rejects_file_over_size_limit, rejects_nesting_over_depth_limit.
```

**Verify:** `scripts/pustman-v1-test.sh api_model postman` → `PASS 8`; tripwire `TRIPWIRE 0`.

### S10 — cURL import and export
**Goal:** a pasted cURL command parses into an `HttpRequest`, and any `HttpRequest` renders as a
cURL command that round-trips.
**Depends on:** S9. **Deploy delta:** none.
**Edits:** `crates/api_model/src/api_model.rs`, `crates/api_model/src/curl.rs`, `crates/api_model/src/curl_tests.rs`.

```
Read docs/PRD.md REQ-HTTP-13 and crates/api_model/src/request.rs. Add api_model::curl:
`parse_curl(&str) -> Result<HttpRequest, CurlError>` using shlex; support -X/--request, -H, -d/
--data/--data-raw/--data-binary/--data-urlencode, -F, -u (→ Basic auth), -b cookie header, -k
(→ settings.verify_tls = Some(false)), -L, --compressed, line continuations (\ and ^); unknown
flags are reported in CurlError::UnsupportedFlag or collected as warnings (choose and record a
Decision). `to_curl(&HttpRequest) -> String` with POSIX single-quote escaping. Tests:
parses_method_headers_and_json_body, parses_basic_auth_flag, parses_form_fields,
insecure_flag_disables_tls_verify, handles_line_continuations, to_curl_roundtrips_through_parse,
to_curl_escapes_single_quotes.
```

**Verify:**
- `scripts/pustman-v1-test.sh api_model curl` → `PASS 7`.
- At merge: `scripts/pustman-v1-verify.sh | tail -1` → `VERIFY OK`.

> **Segment boundary — gate G-DEPS**, in this order: (1) the human installs the packages in S0's deploy
> delta and `scripts/pustman-v1-init.sh --require-gpui` exits 0; (2) the human pushes `pustman-v1/main`
> and CI is green (or waives that promote-check line in writing); (3) promote `release/pustman-v1-1`
> (domain-only: full verify + tripwire, no app smoke). Install comes first because promote-check's env
> validator is `--require-gpui`. Every step below is `[GPUI]` or depends on one.

---

# Phase 2 — HTTP engine [GPUI]

### S11 — `http_engine` core: transport trait, fake, error taxonomy, send/cancel
**Goal:** sending goes through a `Transport` trait, and failures map to the PRD §8.6 taxonomy.
Cancelling drops the task within 50 ms and leaves nothing pending.
**Depends on:** S2, S4, gate G-DEPS. **Deploy delta:** none.
**Edits:** `crates/http_engine/**`.

```
Read the pustman-http-engine skill fully, docs/PRD.md §5.1 REQ-HTTP-12, §8.6, the HTTP ACs,
the gpui-testing skill, and AGENTS.md › GPUI + Async. Create crates/http_engine (deps gpui,
gpui_tokio, api_model, variables, futures, thiserror, url). Types: ResolvedRequest (method, url,
headers in order, body bytes or file ref, settings), Response (status, reason, http_version,
headers in order incl. duplicates, body: ResponseBody, timings: Timings with every phase
Option<Duration>, network: Option<NetworkInfo>, redirects: Vec<RedirectHop>), TransportError
exactly the §8.6 set (InvalidUrl, Dns, Connect, Tls, Timeout, TooManyRedirects{chain}, Proxy,
Protocol, Io, FileNotFound, UnresolvedSecret{names}, Cancelled), each with `user_message()` and
`suggested_fix()` text per §8.6; 4xx/5xx are Ok(Response). `trait Transport: Send + Sync + 'static`
with an async send. `FakeTransport` (test-support feature + test_support.rs) scripted with
responses/errors/delays using GPUI's executor timers. `HttpClient` GPUI entity holding
`pending_send: Option<Task<…>>` — send replaces (and so cancels) any previous one; `cancel()`
drops it and emits a `SendEvent::Cancelled`; never retries. Tests with #[gpui::test]:
send_returns_scripted_response, status_500_is_a_result_not_error, cancel_within_50ms_leaves_no_task,
cancelled_send_is_not_retried, second_send_cancels_first, every_error_kind_has_message_and_fix.
```

**Verify:**
- `scripts/pustman-v1-init.sh --require-gpui` → exit 0.
- `scripts/pustman-v1-test.sh http_engine` → `PASS n` (n ≥ 6).
- `scripts/pustman-v1-tripwire.sh` → `TRIPWIRE 0`.

### S12 — `ReqwestTransport`: settings, manual redirects, TLS toggle, timing spike (D2)
**Goal:** the following work against loopback servers:
- real HTTP sends
- redirect chains recorded hop by hop, with `TooManyRedirects` past the max
- self-signed TLS failing until verification is disabled for that request
- timing and network info filled where measurable

**Depends on:** S11.
**Deploy delta:** none.
**Edits:**
- `crates/http_engine/**`
- `docs/decisions/ADR-0003-timing-phases.md`

```
Read crates/http_engine, the pustman-http-engine skill (gpui_tokio section), docs/PRD.md
REQ-HTTP-09/10, §8.2 (REQ-NET-01..04), the HTTP ACs, ADR template. Use Context7 for reqwest and
hyper. Implement ReqwestTransport (reqwest client on gpui_tokio::Tokio::spawn; the only code that
touches tokio): per-request settings (timeout, http version, verify_tls) building or caching a
client per settings key; redirects handled MANUALLY (reqwest redirect policy none) so each hop's
method, URL, status, headers, timing and followed Location are recorded; max_redirects exceeded →
TooManyRedirects{chain}; follow_original_method honored. Map reqwest/hyper errors to the taxonomy
(Tls when certificate verification fails). SPIKE (D2): determine which of DNS, connect, TLS,
TTFB, download phases reqwest exposes; fill only measurable ones, leave others None — never
estimate; write docs/decisions/ADR-0003-timing-phases.md with findings and the decision (a hyper
connector only inside ReqwestTransport if you choose one). NetworkInfo: remote/local addr, ALPN
version, TLS version + cipher + peer cert chain where available. Test servers in a NEW file
crates/http_engine/src/test_server.rs (tokio + hyper loopback; rcgen self-signed TLS), declared
from http_engine.rs behind the test-support feature — S11's test_support.rs is read-only now. Tests in
crates/http_engine/tests/reqwest_transport.rs: sends_post_with_json_body_to_loopback,
six_redirects_with_max_five_fail_with_chain_of_six, redirect_chain_records_each_hop,
self_signed_fails_with_tls_error, self_signed_succeeds_when_verify_disabled_for_request,
timeout_maps_to_timeout_error, connect_refused_maps_to_connect_error,
timings_are_none_or_measured_never_estimated.
```

**Verify:**
- `scripts/pustman-v1-test.sh http_engine reqwest_transport` → `PASS 8`.
- `test -f docs/decisions/ADR-0003-timing-phases.md && grep -c 'Status: Accepted' docs/decisions/ADR-0003-timing-phases.md` → `1`.


### S13a — Variable resolution of URL, params, headers and body text
**Goal:** a pure function turns an `HttpRequest` plus a scope stack into resolved URL, params,
headers and body text. Unresolved names come back as a warning, and dynamic values stay stable within
one send.
**Depends on:** S11, S12. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/pipeline.rs`, `crates/http_engine/src/pipeline_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`), `crates/http_engine/Cargo.toml` (append deps).

```
Read crates/http_engine (request.rs ResolvedRequest), crates/variables, docs/PRD.md REQ-VAR-01/02/07.
Add src/pipeline.rs + src/pipeline_tests.rs: `resolve_request(request: &HttpRequest, scopes:
&ScopeStack, dynamic: &mut DynamicValues) -> (ResolvedRequest, Vec<Warning>)` resolving URL,
query params, headers and raw/urlencoded body text through crates/variables. Define `pub enum
Warning { UnresolvedVariables(Vec<String>), TlsVerifyDisabled }` here. One DynamicValues per send:
the same {{$guid}} in two fields resolves to the same value. Auth, secrets and body encoding are
NOT part of this step (S13b, S13c, S14a). Pure: no IO, no logging of any value. Do not edit
client.rs, request.rs or reqwest_transport.rs. Tests: resolves_url_headers_and_body,
unresolved_variable_warns_but_sends, dynamic_guid_same_across_fields_in_one_send.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine pipeline::` → `PASS 3`.

### S13b — Effective auth applied to a request (Basic, Bearer, API key)
**Goal:** a pure function applies the effective auth (own or inherited) to resolved headers and query.
**Depends on:** S11, S12. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/request_auth.rs`, `crates/http_engine/src/request_auth_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`).

```
Read crates/api_model/src/auth.rs (effective_auth), crates/http_engine/src/request.rs, docs/PRD.md
REQ-MODEL-01, REQ-HTTP-06 (M1 subset only: Inherit, None, Basic, Bearer, API key in header or
query). Add src/request_auth.rs + src/request_auth_tests.rs: `apply_auth(resolved: &mut
ResolvedRequest, own: &Auth, ancestors: &[&Auth], resolve: impl Fn(&str) -> String)` — Inherit walks
the ancestors (request → folder → collection), None adds nothing, Basic sets `Authorization: Basic
<base64>`, Bearer sets `Authorization: Bearer <token>`, API key adds a header or a query param. Auth
field values go through `resolve` so {{vars}} work. Pure: no IO, no logging. Do not edit client.rs or
pipeline.rs. Tests: basic_auth_header_applied, bearer_inherited_from_collection, api_key_in_query.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine request_auth::` → `PASS 3`.

### S13c — `prepare` + `HttpClient::send` wiring; unresolved secret fails before the network
**Goal:** `HttpClient::send` prepares every request (variables, auth, secrets) before the transport.
A secret with no local value fails as `UnresolvedSecret` and the transport sees zero calls.
**Depends on:** S13a, S13b, S12, S7. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/http_engine/src/prepare.rs`, `crates/http_engine/src/prepare_tests.rs`,
`crates/http_engine/src/client.rs`, `crates/http_engine/src/http_engine.rs` (append `mod`),
`crates/http_engine/Cargo.toml` (append deps).

```
Read crates/http_engine (client.rs, pipeline.rs, request_auth.rs), crates/secrets (SecretStore,
MemorySecretStore), docs/PRD.md §8.6 UnresolvedSecret. Add src/prepare.rs + src/prepare_tests.rs:
`prepare(request, ancestors: &[&Auth], scopes, secrets: &dyn SecretStore, dynamic: &mut
DynamicValues) -> Result<Prepared, TransportError>` where Prepared = { resolved: ResolvedRequest,
warnings: Vec<Warning>, secrets_used: SecretValues (for redaction downstream) }. Look up every
secret variable the request references; any with no local value → Err(UnresolvedSecret{names})
before anything else runs. Then resolve_request (pipeline.rs) and apply_auth (request_auth.rs).
Wire HttpClient::send to call prepare, then the transport. Never log a value; secrets_used carries
them only for redaction. Test: missing_secret_fails_before_network (assert FakeTransport saw zero
calls).
```

**Verify:** `scripts/pustman-v1-test.sh http_engine prepare::` → `PASS 1`.

### S14b — File references resolve from the workspace root
**Goal:** body file references resolve relative to the workspace root. A missing file is a
`FileNotFound` error naming the path.
**Depends on:** S11, S12. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/file_ref.rs`, `crates/http_engine/src/file_ref_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`).

```
Read crates/http_engine/src/error.rs, docs/PRD.md REQ-HTTP-04/05. Add src/file_ref.rs +
src/file_ref_tests.rs: `resolve_file_ref(workspace_root: &Path, reference: &str) -> Result<PathBuf,
TransportError>` — relative → workspace_root.join, absolute kept, missing → FileNotFound{path}. The
existence check runs where the caller runs it (the body encoder calls it from a background task,
never the foreground). Tests: relative_file_ref_resolves_from_workspace_root,
missing_file_is_file_not_found_error.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine file_ref::` → `PASS 2`.

### S14c — Large responses spill to a temp file
**Goal:** a response over the limit (10 MB by default) keeps an in-memory prefix and streams the rest
to a temp file, which is deleted when its owning handle drops.
**Depends on:** S12. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/spill.rs`, `crates/http_engine/src/spill_tests.rs`,
`crates/http_engine/src/response.rs`, `crates/http_engine/src/reqwest_transport.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`), `crates/http_engine/Cargo.toml` (append deps).

```
Read crates/http_engine (response.rs ResponseBody, reqwest_transport.rs), docs/PRD.md REQ-RESP-06,
REQ-NFR-03. Add src/spill.rs + src/spill_tests.rs: a writer that keeps the first N bytes (N from
RequestSettings / global default, 10 MB) as `ResponseBody::Prefix` and streams the remainder to a
tempfile in the OS temp dir → `ResponseBody::Spilled{prefix, path, total_len}`; the temp file is
removed when the owning handle drops. Make ReqwestTransport stream the body through it (no whole-body
buffering). Do not change redirect, TLS or timing behavior from S12. Tests:
response_over_limit_spills_to_temp_file, temp_file_removed_on_drop.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine spill::` → `PASS 2`.

### S14a — Request body modes and auto headers
**Goal:** all M1 body modes send correctly. Auto headers are added, listed in the resolved request,
and a user header with the same name wins.
**Depends on:** S13c, S14b, S14c. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/body.rs`, `crates/http_engine/src/body_tests.rs`,
`crates/http_engine/src/request.rs`, `crates/http_engine/src/prepare.rs`,
`crates/http_engine/src/reqwest_transport.rs`, `crates/http_engine/src/http_engine.rs` (append `mod`),
`crates/http_engine/Cargo.toml` (append deps).

```
Read crates/http_engine (prepare.rs, file_ref.rs, reqwest_transport.rs), docs/PRD.md REQ-HTTP-03/04/05.
Add src/body.rs + src/body_tests.rs: encode Body modes — form-data with text + file fields and
per-field content type via reqwest multipart; x-www-form-urlencoded; raw with Content-Type from the
language unless the user set one; binary file. File fields use resolve_file_ref. Auto headers
(Content-Type, Content-Length, User-Agent "Pustman/<version>", Accept "*/*", Accept-Encoding, Host)
are listed in a new `ResolvedRequest.auto_headers` so the UI can show them; a user header with the
same name wins. prepare() fills them; ReqwestTransport sends the encoded body. File reads happen on
a background task. Tests: json_raw_body_sets_content_type, user_content_type_overrides_auto,
urlencoded_body_encodes_pairs, form_data_file_field_sent.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine body::` → `PASS 4`.

### S15a — Per-workspace cookie jar and its management API
**Goal:** a `CookieJar` stores cookies with domain and path scoping and offers the list, add, edit,
delete and clear-by-domain API the Manage Cookies UI needs.
**Depends on:** S11, S12. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/cookies.rs`, `crates/http_engine/src/cookies_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`), `crates/http_engine/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-HTTP-08, REQ-WSP-06. Check the cookie_store crate's current API with Context7
before using it. Add src/cookies.rs + src/cookies_tests.rs: a per-workspace `CookieJar` wrapping
cookie_store: `store_response_cookies(url, set_cookie_headers)`, `cookie_header_for(url) ->
Option<String>`, and `list`, `add`, `edit`, `delete`, `clear_domain`. No transport wiring (S15c) and
no persistence (S15b) here. Never log cookie values. Tests: cookie_scoped_by_domain_and_path,
delete_cookie_removes_it.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine cookies::` → `PASS 2`.

### S15b — Cookie jar persists through `storage`
**Goal:** the jar loads from and saves to the `storage` cookies table on a background task. Workspace
files never hold cookies.
**Depends on:** S15a, S6. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/cookie_persistence.rs`,
`crates/http_engine/src/cookie_persistence_tests.rs`, `crates/http_engine/src/http_engine.rs`
(append `mod`), `crates/http_engine/Cargo.toml` (append deps).

```
Read crates/storage (cookies table), crates/http_engine/src/cookies.rs, docs/PRD.md REQ-WSP-06.
Add src/cookie_persistence.rs + src/cookie_persistence_tests.rs: `load_jar(store, workspace_id)`
and `save_jar(store, workspace_id, &CookieJar)`, both run on a background task (never the
foreground). Cookies are local state: they go to SQLite, never to workspace files. Test:
jar_persists_and_reloads_from_storage.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine cookie_persistence::` → `PASS 1`.

### S15c — Cookies applied by the transport, including redirect hops
**Goal:** `ReqwestTransport` captures `Set-Cookie` and applies the jar on every hop, including
redirects. The applied `Cookie` header appears in the resolved request.
**Depends on:** S15a, S14a. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/cookie_transport.rs`,
`crates/http_engine/src/cookie_transport_tests.rs`, `crates/http_engine/src/reqwest_transport.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod`).

```
Read crates/http_engine (cookies.rs, reqwest_transport.rs manual redirects), docs/PRD.md REQ-HTTP-08,
REQ-NET-04. Add src/cookie_transport.rs + src/cookie_transport_tests.rs: ReqwestTransport holds a
shared CookieJar handle and applies it manually on each hop (reqwest's own cookie store stays off),
so redirect hops capture and apply cookies too; the Cookie header actually sent is recorded in the
resolved request. Do not edit client.rs. Never log cookie values. Tests (loopback servers from
test_server.rs): set_cookie_captured_and_sent_on_next_request, redirect_hop_cookie_applied.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine cookie_transport::` → `PASS 2`.

### S16a — History recording with redaction and a 1 MB body cap
**Goal:** every send (success, error or cancel) writes a history row built from the redacted,
pre-resolution request plus a response summary with the body capped at 1 MB.
**Depends on:** S13c, S14c, S3, S6. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/http_engine/src/history.rs`, `crates/http_engine/src/history_tests.rs`,
`crates/http_engine/src/client.rs`, `crates/http_engine/src/http_engine.rs` (append `mod`),
`crates/http_engine/Cargo.toml` (append deps).

```
Read docs/PRD.md §8.5 (REQ-HIST-01..03), REQ-NFR-04, crates/api_model/src/redact.rs, crates/storage
history repo, crates/http_engine/src/client.rs. Add src/history.rs + src/history_tests.rs: after
each send (success, error or cancel) build a HistoryRecord from the PRE-resolution HttpRequest (so
{{vars}} stay as written) + response summary (status, timings, sizes, headers redacted) + body
prefix ≤ 1 MB; run redact on every text field with the send's secrets_used plus header rules; write
on a background task. Hook it into HttpClient::send. Retention is S16b. Never log values. Tests:
history_stores_pre_resolution_request, history_never_contains_secret_value (scan the whole SQLite
file bytes for the secret), history_body_capped_at_one_megabyte, cancelled_send_recorded_as_cancelled.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine history::` → `PASS 4`.

### S16b — History retention
**Goal:** history rows older than the retention setting (30 days by default) are pruned at most once
per app session.
**Depends on:** S16a. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/retention.rs`, `crates/http_engine/src/retention_tests.rs`,
`crates/http_engine/src/history.rs`, `crates/http_engine/src/http_engine.rs` (append `mod`).

```
Read docs/PRD.md REQ-HIST-04, crates/http_engine/src/history.rs, crates/storage history repo. Add
src/retention.rs + src/retention_tests.rs: prune by the retention setting (default 30 days) at most
once per app session, triggered from the history writer on a background task. Test:
retention_prunes_entries_older_than_setting.
```

**Verify:**
- `scripts/pustman-v1-test.sh http_engine retention::` → `PASS 1`.
- At merge: `scripts/pustman-v1-verify.sh | tail -1` → `VERIFY OK`.

---

# Phase 3 — App shell (M0) [GPUI]

### S17 — `settings` crate
**Goal:** layered settings (defaults JSON → user `settings.json`) are typed, observable and
validated. A broken user file surfaces an error and keeps the last good values.
**Depends on:** S1, gate G-DEPS. **Deploy delta:** none.
**Edits:**
- `crates/settings/**`
- `assets/settings/default.json`

```
Load the pustman-settings-persistence skill. Read docs/PRD.md REQ-UI-08, REQ-HTTP-09 (global
request defaults), REQ-HIST-04 (retention), REQ-CON-02 (console cap). Write our own SettingsStore
(GPL Zed settings — patterns only): a GPUI Global holding defaults from assets/settings/default.json
(embedded) merged with the user file (platform config dir, via a `paths` helper in this crate);
typed `Settings` trait with `register::<T>(cx)` and `T::get_global(cx)`; file reload on change
via a background watcher; parse errors produce a SettingsError that callers show as a toast and
the last good values stay. default.json keys: theme (light/dark/system), ui_font_size,
buffer_font_size, request defaults (timeout_ms 30000, follow_redirects true, max_redirects 10,
verify_tls true, response_pretty_limit_mb 10), history_retention_days 30, console_max_entries 5000.
Tests (#[gpui::test]): defaults_load, user_value_overrides_default, invalid_user_file_keeps_last_good
_values_and_reports_error, observer_fires_on_change.
```

**Verify:** `scripts/pustman-v1-test.sh settings` → `PASS 4`; `python3 -m json.tool assets/settings/default.json >/dev/null` exits 0.

### S18 — `theme` crate with bundled light and dark themes
**Goal:** `cx.theme()` returns typed colors from bundled JSON themes. The app follows the OS
appearance by default, and both themes meet 4.5:1 text contrast.
**Depends on:** S17. **Deploy delta:** none.
**Edits:**
- `crates/theme/**`
- `assets/themes/**`

```
Load the rust-gpui-ui-ux skill (theme sections) and figma-tokens-to-theme for the token names.
Read docs/PRD.md REQ-UI-06, REQ-NFR-06. Write our own theme crate (Zed's is GPL — patterns only):
Theme { name, appearance, colors: ThemeColors, status: StatusColors (success/warning/error/info +
HTTP status classes + method colors) }, an ActiveTheme trait (`cx.theme()`), a ThemeRegistry
loading assets/themes/pustman-light.json and pustman-dark.json, selection from the `theme`
setting with `system` following window appearance changes. Tests: both_bundled_themes_parse,
system_setting_follows_appearance, text_contrast_at_least_4_5_to_1 (compute WCAG contrast for
text/background pairs listed in a const table).
```

**Verify:** `scripts/pustman-v1-test.sh theme` → `PASS 3`.


### S19a — `ui` crate root, prelude and base components
**Goal:** the `ui` crate builds with a `prelude` holding the base components: Button, IconButton,
Label, Icon, Tab/TabBar, Table, Toggle, Tooltip, Modal frame, Toast, Badge and Divider. It also ships
the 16x16 icon set and its license.
**Depends on:** S18. **Deploy delta:** none.
**Edits:**
- `crates/ui/Cargo.toml`, `crates/ui/src/ui.rs`, `crates/ui/src/prelude.rs`
- `crates/ui/src/components/*.rs`, except `context_menu.rs`, `list.rs`, `tab_bar_tests.rs` and `toast_tests.rs`
- `assets/icons/**`

```
Load the rust-gpui-ui-ux skill (components, spacing, accessibility references). Read docs/PRD.md §3,
REQ-UI-07, REQ-NFR-06. Build crates/ui with a `prelude` module. Every component is RenderOnce or an
Entity as the skill prescribes, reads colors only from cx.theme(), takes a stable ElementId, is
keyboard-focusable where interactive, and has an accessible label. Components here: Button,
IconButton, Label, Icon, Tab/TabBar, Table, Toggle, Tooltip, Modal frame, Toast, Badge, Divider.
ContextMenu, the virtual List and the story view are S19b; the TabBar and Toast tests are S19c.
Icons: 16x16 SVGs (Lucide-derived, ISC license noted in assets/icons/LICENSE) for the set PRD M0–M2
needs, embedded via an AssetSource. Do NOT generate raster images. No tests in this step: it is the
base S19b/S19c test against. If you are resuming the S19 worktree, keep its work and trim it to
this scope.
```

**Verify:**
- `cargo build -p ui` → exit 0.
- `cargo clippy -p ui --all-targets -- -D warnings` → exit 0.
- `test -f assets/icons/LICENSE && echo ok` → `ok`.

### S19b — ContextMenu, virtual List and the story view
**Goal:** ContextMenu and a virtualized List join the prelude, and a story view renders every
component in both themes.
**Depends on:** S19a. **Deploy delta:** none.
**Edits:**
- `crates/ui/src/components/context_menu.rs`, `crates/ui/src/components/list.rs`, `crates/ui/src/components/list_tests.rs`
- `crates/ui/src/story.rs`, `crates/ui/src/story_tests.rs`
- `crates/ui/src/ui.rs`, `crates/ui/src/prelude.rs`, `crates/ui/src/components.rs` (append `mod`/`pub use` lines only)

```
Load rust-gpui-ui-ux (components, lists). Read crates/ui (S19a). Add ContextMenu (keyboard
navigable, Esc closes, accessible labels) and a virtualized List built on GPUI's uniform_list
(renders only visible rows, stable ids from item keys). Add src/story.rs: a test view rendering
every prelude component. Theme colors only via cx.theme(). Tests (#[gpui::test] with
VisualTestContext): story_tests.rs: all_components_render_in_both_themes,
button_activates_on_enter_and_space; components/list_tests.rs: virtual_list_renders_only_visible_rows.
Do not edit any other component's file. Report which ux-checklist.md items you verified.
```

**Verify:**
- `scripts/pustman-v1-test.sh ui story::` → `PASS 2`.
- `scripts/pustman-v1-test.sh ui components::list::` → `PASS 1`.

### S19c — TabBar keyboard cycling and Toast auto-dismiss
**Goal:** TabBar cycles with the keyboard, and Toast dismisses itself after its timeout on a GPUI timer.
**Depends on:** S19a. **Deploy delta:** none.
**Edits:**
- `crates/ui/src/components/tab_bar_tests.rs`, `crates/ui/src/components/toast_tests.rs`
- `crates/ui/src/components/tab_bar.rs`, `crates/ui/src/components/toast.rs` (only the `#[cfg(test)] #[path]` line, plus behavior when the tests need it)

```
Load rust-gpui-ui-ux and gpui-testing. Read crates/ui/src/components/{tab_bar,toast}.rs. Add tests,
and the behavior they need when it is missing: tab_bar_tests.rs: tab_bar_cycles_with_keyboard;
toast_tests.rs: toast_auto_dismisses_after_timeout (advance the GPUI test clock; never
smol::Timer or thread::sleep). Toast timers are Tasks stored on the entity. Do not touch
context_menu.rs, list.rs or story.rs.
```

**Verify:**
- `scripts/pustman-v1-test.sh ui components::tab_bar::` → `PASS 1`.
- `scripts/pustman-v1-test.sh ui components::toast::` → `PASS 1`.

### S20a — `text_input`: buffer and single-line TextInput
**Goal:** a single-line TextInput supports typing, cursor movement by character and word,
shift-selection, IME through `EntityInputHandler`, a placeholder and read-only mode. Enter emits
`Submitted`, and edits emit `Changed`.
**Depends on:** S19a. **Deploy delta:** none.
**Edits:** `crates/text_input/Cargo.toml`, `crates/text_input/src/text_input.rs`,
`crates/text_input/src/buffer.rs`, `crates/text_input/src/input.rs`, `crates/text_input/src/input_tests.rs`.

```
Load the rust-gpui-ui-ux skill (text input reference) and read GPUI's examples/input.rs at the
pinned rev (Apache — may be adapted). src/buffer.rs: a rope or gap buffer so typing never
reallocates the whole text per keystroke (record a Decision). src/input.rs: TextInput entity —
cursor by char/word, shift selection, select-all, IME via EntityInputHandler, placeholder,
read-only, `Changed`/`Submitted` events. Clipboard and undo are S20b; TextArea and highlights are
S20c. Tests (input_tests.rs): typing_inserts_at_cursor, word_motion_and_shift_selection,
enter_in_single_line_emits_submitted.
```

**Verify:** `scripts/pustman-v1-test.sh text_input input::` → `PASS 3`.

### S20b — Clipboard and undo/redo
**Goal:** copy, cut and paste work, and undo/redo restores text and selection.
**Depends on:** S20a. **Deploy delta:** none.
**Edits:** `crates/text_input/src/editing.rs`, `crates/text_input/src/editing_tests.rs`,
`crates/text_input/src/input.rs` (action registration), `crates/text_input/src/text_input.rs` (append `mod`).

```
Read crates/text_input (buffer.rs, input.rs). src/editing.rs: an undo stack of edit transactions
(coalescing consecutive typing), copy/cut/paste through cx clipboard, paste replaces the selection.
Register the actions on TextInput. Tests (editing_tests.rs): undo_redo_restores_text,
paste_replaces_selection.
```

**Verify:** `scripts/pustman-v1-test.sh text_input editing::` → `PASS 2`.

### S20c — Multi-line TextArea and highlight ranges
**Goal:** a multi-line TextArea (soft wrap, optional line numbers) shares the buffer and editing.
`set_highlights(Vec<(Range<usize>, HighlightStyle)>)` survives edits made before a highlighted span.
**Depends on:** S20b. **Deploy delta:** none.
**Edits:** `crates/text_input/src/text_area.rs`, `crates/text_input/src/highlights.rs`,
`crates/text_input/src/highlights_tests.rs`, `crates/text_input/src/input.rs`,
`crates/text_input/src/text_input.rs` (append `mod`).

```
Read crates/text_input. src/text_area.rs: TextArea entity — line motion, soft wrap, optional line
numbers, the same editing actions as TextInput. src/highlights.rs: `set_highlights` on both inputs;
ranges shift with edits before them (anchors). Tests (highlights_tests.rs):
highlights_survive_edits_before_them.
```

**Verify:** `scripts/pustman-v1-test.sh text_input highlights::` → `PASS 1`.

### S21 — `pustman_actions` + complete default keymaps (D10)
**Goal:** every PRD action is declared in one crate, and the default keymaps bind every PRD
keybinding on all three platforms.
**Depends on:** S1, gate G-DEPS. **Deploy delta:** none.
**Edits:**
- `crates/pustman_actions/**`
- `assets/keymaps/**`

```
Load the gpui-actions-keymaps skill. Read docs/PRD.md §3 keybinding table and every REQ that names
an action. Create crates/pustman_actions with `actions!` namespaces: workspace (NewRequest,
NewTab, CloseTab, NextTab, PreviousTab, ToggleSidebar, ToggleConsole, OpenWorkspace,
CreateWorkspace, OpenRecent, Save, SaveAll), request (Send, Cancel, FocusUrlBar, CopyAsCurl,
PasteCurl, ToggleTlsVerify), response (Find, SaveBody, CopyBody), palette (Toggle), quick_open
(Toggle), environment (SelectEnvironment, QuickLook), history (Focus), console (Clear, Pause),
cookies (Manage), import_export (Import, Export). Write assets/keymaps/default-linux.json,
default-windows.json (ctrl) and default-macos.json (cmd) binding every PRD keybinding with the
right context (e.g. Send in "RequestEditor", Cancel=escape in "RequestEditor"). Add a loader
`load_default_keymap(cx)`. Tests: every_action_name_resolves (iterate all names through
cx.build_action), every_keymap_entry_parses_on_all_platforms, prd_bindings_present (table of the
PRD §3 bindings asserted against each platform file), no_duplicate_bindings_in_same_context.
```

**Verify:** `scripts/pustman-v1-test.sh pustman_actions` → `PASS 4`.


### S22a — `workspace` crate: Workspace, docks and status bar
**Goal:** a `Workspace` entity renders a left dock, a center pane, a bottom dock and a status bar
(`add_left_item` and `add_right_item`). `ToggleSidebar` and `ToggleConsole` hide and show the docks.
**Depends on:** S19a, S21. **Deploy delta:** none.
**Edits:** `crates/workspace/Cargo.toml`, `crates/workspace/src/workspace.rs`,
`crates/workspace/src/workspace_tests.rs`, `crates/workspace/src/pane.rs`,
`crates/workspace/src/dock.rs`, `crates/workspace/src/status_bar.rs`.

```
Load rust-gpui-ui-ux and pustman-feature-crate. Read AGENTS.md (init order), docs/PRD.md §3
(REQ-UI-01/02/04/07). Write our own workspace crate (Zed's is GPL — patterns only): Workspace entity
with left Dock, center Pane (a minimal pane holding items; tabs are S22b), bottom Dock, StatusBar
with add_left_item/add_right_item; `pub fn init(cx)` registering ToggleSidebar/ToggleConsole
handlers. Do not touch crates/pustman. Tests (workspace_tests.rs):
workspace_opens_with_docks_and_status_bar, toggle_sidebar_hides_left_dock.
```

**Verify:**
- `scripts/pustman-v1-test.sh workspace workspace_opens_with_docks_and_status_bar` → `PASS 1`.
- `scripts/pustman-v1-test.sh workspace toggle_sidebar_hides_left_dock` → `PASS 1`.

### S22b — Pane tabs and the `Item` trait
**Goal:** the center pane shows tabs (reorder, close, pin, dirty dot) for items that implement the
`Item` trait. A Welcome item is the empty state. `NewTab`, `CloseTab`, `NextTab` and `PreviousTab` work.
**Depends on:** S22a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/item.rs`, `crates/workspace/src/pane_tabs.rs`,
`crates/workspace/src/pane_tabs_tests.rs`, `crates/workspace/src/welcome.rs`,
`crates/workspace/src/pane.rs`, `crates/workspace/src/workspace.rs` (append `mod`/init lines only).

```
Read crates/workspace. src/item.rs: `Item` trait for tab contents (title, dirty, serialize_draft hook
returning Option<String>, default None). src/pane_tabs.rs: tab strip on Pane using ui TabBar —
reorder, close, pin, dirty dot; handlers for NewTab/CloseTab/NextTab/PreviousTab. src/welcome.rs: a
Welcome item as the empty state (real UI, not a stub). Tests (pane_tabs_tests.rs):
close_tab_action_closes_active_tab, next_tab_cycles.
```

**Verify:** `scripts/pustman-v1-test.sh workspace pane_tabs::` → `PASS 2`.

### S22c — `Panel` trait and dock registration
**Goal:** feature crates register their own dock panels through a `Panel` trait, so a new panel
never needs a workspace edit.
**Depends on:** S22a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/panel.rs`, `crates/workspace/src/panel_tests.rs`,
`crates/workspace/src/dock.rs`, `crates/workspace/src/workspace.rs` (append `mod`/init lines only).

```
Read crates/workspace (dock.rs). src/panel.rs: `Panel` trait (id, title, icon, position Left/Bottom,
focus) and `Workspace::register_panel` storing panels in their dock; dock shows a selector entry per
panel. Tests (panel_tests.rs): panel_registration_adds_dock_entry.
```

**Verify:** `scripts/pustman-v1-test.sh workspace panel::` → `PASS 1`.

### S22d — Toasts and the modal layer
**Goal:** `workspace.show_toast(...)` shows a toast that dismisses itself, and a `ModalLayer` hosts
one modal at a time.
**Depends on:** S22a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/toast_layer.rs`, `crates/workspace/src/toast_layer_tests.rs`,
`crates/workspace/src/modal_layer.rs`, `crates/workspace/src/status_bar.rs`,
`crates/workspace/src/workspace.rs`.

```
Read crates/workspace, crates/ui Toast. src/toast_layer.rs: Workspace::show_toast(message, level,
optional action) using ui Toast, timers as stored Tasks. src/modal_layer.rs: ModalLayer
(toggle_modal, dismiss on Esc and click-outside, focus restore). Render both from workspace.rs.
Tests (toast_layer_tests.rs): toast_shows_and_dismisses.
```

**Verify:** `scripts/pustman-v1-test.sh workspace toast_layer::` → `PASS 1`.

### S22e — `pustman` binary becomes library + window (wire step)
**Goal:** `cargo run -p pustman` opens the workspace window. `crates/pustman` becomes a library:
`src/pustman.rs` holds `pub fn init(cx)` and the init order, and `src/main.rs` only opens the
window. Startup time is traced.
**Depends on:** S22a, S22d, S6, S17, S18, S21. **Deploy delta:** none.
**Edits:**
- `crates/pustman/Cargo.toml` (hotspot)
- `crates/pustman/src/pustman.rs` (hotspot)
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/tests/wire_shell.rs`

```
Load pustman-feature-crate. Read AGENTS.md (init order), docs/PRD.md REQ-NFR-02, the "Smaller steps"
section of this doc (convention 4). crates/pustman: [lib] path = "src/pustman.rs" with `pub fn
init(cx: &mut App)` calling settings → theme → ui → pustman_actions keymap → storage (open
LocalStore in the platform data dir on a background task; failure → toast and in-memory store) →
workspace, one line per crate so later wire steps append. src/main.rs: run the app, call
pustman::init, open the window; with PUSTMAN_STARTUP_TRACE=1 print `startup_ms=<n>` to stderr at the
first painted frame. tests/wire_shell.rs (#[gpui::test]): wire_shell_init_opens_workspace_with_docks.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_shell` → `PASS 1`.
- `cargo build -p pustman` → exit 0.
- E2E (display): `PUSTMAN_STARTUP_TRACE=1 cargo run --release -p pustman` prints `startup_ms=<n>`, and the window shows the docks and status bar.

### S23a — Layout and tab restore
**Goal:** dock sizes, dock visibility and open tabs (order, pinned, active, draft) are restored
after a restart. A tab whose backing file vanished is reported, not restored.
**Depends on:** S22b, S22c, S22d, S6. **Deploy delta:** none.
**Edits:** `crates/workspace/src/restore.rs`, `crates/workspace/src/restore_tests.rs`,
`crates/workspace/src/workspace.rs` (append `mod`/init lines only), `crates/workspace/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-UI-01/02, REQ-NFR-05, crates/workspace, crates/storage (open_tabs, layout).
src/restore.rs: serialize layout and open tabs (item ids, order, pinned, active, draft_json via
Item::serialize_draft) to storage on change, debounced, on a background task; restore on window
open; vanished items → toast, not restored. Tests (restore_tests.rs): layout_restored_after_reopen,
tabs_restored_in_order_with_active_and_pinned, missing_item_reported_not_restored.
```

**Verify:** `scripts/pustman-v1-test.sh workspace restore::` → `PASS 3`.

### S23b — Draft autosave
**Goal:** dirty drafts autosave every 2 s on a GPUI timer task stored on the Workspace.
**Depends on:** S23a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/autosave.rs`, `crates/workspace/src/autosave_tests.rs`,
`crates/workspace/src/workspace.rs` (append `mod`/init lines only).

```
Read crates/workspace (restore.rs). src/autosave.rs: a 2 s repeating GPUI timer Task stored on the
Workspace writes dirty drafts through the restore path on a background task. Tests
(autosave_tests.rs): draft_autosaved_after_two_seconds (advance the test clock).
```

**Verify:** `scripts/pustman-v1-test.sh workspace autosave::` → `PASS 1`.

### S24a — Picker and fuzzy scorer
**Goal:** a reusable modal Picker (TextInput query, up/down/enter/escape) ranks items with our own
fuzzy scorer.
**Depends on:** S19a, S20a. **Deploy delta:** none.
**Edits:** `crates/command_palette/Cargo.toml`, `crates/command_palette/src/command_palette.rs`,
`crates/command_palette/src/picker.rs`, `crates/command_palette/src/picker_tests.rs`,
`crates/command_palette/src/fuzzy.rs`, `crates/command_palette/src/fuzzy_tests.rs`.

```
Load rust-gpui-ui-ux and gpui-actions-keymaps. Write our own Picker (Zed's picker is GPL — patterns
only): `PickerDelegate` trait (items, match, confirm, dismiss), modal view with a TextInput query and
a keyboard-driven list. src/fuzzy.rs: a simple scorer where prefix and word-start matches rank first.
Keep Picker generic so S34a, S41b and S46 can reuse it (record a Decision on where it lives). Tests:
picker_tests.rs: escape_dismisses; fuzzy_tests.rs: fuzzy_query_ranks_prefix_first.
```

**Verify:**
- `scripts/pustman-v1-test.sh command_palette picker::` → `PASS 1`.
- `scripts/pustman-v1-test.sh command_palette fuzzy::` → `PASS 1`.

### S24b — Command palette delegate
**Goal:** `Ctrl/Cmd-Shift-P` opens the palette with every available action and its binding from the
active keymap. Choosing an entry dispatches the action.
**Depends on:** S24a, S21, S22d. **Deploy delta:** none.
**Edits:** `crates/command_palette/src/palette.rs`, `crates/command_palette/src/palette_tests.rs`,
`crates/command_palette/src/command_palette.rs` (append `mod`/init lines only), `crates/command_palette/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-UI-05, crates/pustman_actions, crates/command_palette (picker.rs). src/palette.rs:
a PickerDelegate listing every registered action (humanized name + binding), opened through the
workspace ModalLayer by the palette toggle action. Do not touch crates/pustman. Tests
(palette_tests.rs): palette_lists_every_registered_action_with_binding, enter_dispatches_selected_action.
```

**Verify:** `scripts/pustman-v1-test.sh command_palette palette::` → `PASS 2`.

### S24w — Wire the command palette
**Goal:** the running app opens the palette with `Ctrl/Cmd-Shift-P`.
**Depends on:** S24b, S22e. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_command_palette.rs`.

```
Append `command_palette::init(cx)` to pustman::init after workspace, and add the dependency. Test
(tests/wire_command_palette.rs): wire_command_palette_toggle_opens_modal.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_command_palette` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S25 — M0 SIGN-OFF (HUMAN)
**Goal:** the M0 exit criteria are proven on real machines.
**Depends on:** S19b, S19c, S20c, S23b, S24w. **Deploy delta:** none. **Edits:** none.

```
Human validation, not code. On Linux (X11 and Wayland if available) and macOS:
1) `PUSTMAN_STARTUP_TRACE=1 cargo run --release -p pustman` 5 times → median startup_ms < 1000.
2) Open 3 tabs, resize/hide docks, quit, relaunch → tabs, active tab, dock sizes restored.
3) Palette lists every action with its binding; Ctrl-T/W/Tab, Ctrl-\, Ctrl-Alt-C work.
4) `cargo test -p secrets -- --ignored` passes (live keychain).
5) Theme follows OS appearance. Record ux-checklist items verified.
Record results + go/no-go in the S25 step record via the driver. Then /promote → release/pustman-v1-2.
```

**Verify:** the human records `M0 GO` with the 5 results in the log. CI is green on Linux and macOS at the S24w merge SHA.

---

# Phase 4 — HTTP UI + debugging (M1) [GPUI]


### S26a — `request_editor` crate: Item, URL bar, method picker, NewRequest
**Goal:** `NewRequest` opens a request tab with a method picker (standard verbs plus a validated
custom token) and a URL input. `Ctrl-L` focuses the URL bar, and the tab shows a dirty dot.
**Depends on:** S25, S15c, S16b. **Deploy delta:** none.
**Edits:** `crates/request_editor/Cargo.toml`, `crates/request_editor/src/request_editor.rs`,
`crates/request_editor/src/url_bar.rs`, `crates/request_editor/src/method.rs`,
`crates/request_editor/src/method_tests.rs`, `crates/request_editor/src/new_request.rs`,
`crates/request_editor/src/new_request_tests.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-HTTP-01, REQ-UI-02/03, crates/workspace (Item trait),
crates/text_input. src/request_editor.rs: RequestEditor Item holding the HttpRequest draft, fields
`pending_send: Option<Task<()>>`, `inline_error`, and a status line area (filled by S26b); `pub fn
init`. src/url_bar.rs: method dropdown + URL TextInput with `url()`, `set_url()` and a `UrlChanged`
event. src/method.rs: custom method token validation (RFC 9110 token). src/new_request.rs: handler
for workspace::NewRequest and request::FocusUrlBar. Do not touch crates/pustman. Tests:
method_tests.rs: custom_method_rejects_invalid_token; new_request_tests.rs:
new_request_action_opens_tab.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor method::` → `PASS 1`.
- `scripts/pustman-v1-test.sh request_editor new_request::` → `PASS 1`.

### S26b — Send and cancel with a status line and inline errors
**Goal:** `Ctrl-Enter` sends through `http_engine` and `Esc` cancels. Send toggles to Cancel while a
request is in flight. The status line shows status (colored by class), reason, time and size, and a
failure shows an inline message with its suggested fix.
**Depends on:** S26a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/send.rs`, `crates/request_editor/src/send_tests.rs`,
`crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-HTTP-12, REQ-RESP-01, crates/http_engine (HttpClient, FakeTransport,
TransportError user_message/suggested_fix). src/send.rs: request::Send / request::Cancel handlers;
the send Task is stored in pending_send (replacing it cancels); results update the status line;
errors render inline (never only logged). Tests (send_tests.rs, FakeTransport):
ctrl_enter_sends_via_fake_transport, esc_cancels_in_flight_send,
send_button_toggles_to_cancel_while_in_flight, error_shows_inline_message_and_fix.
Report ux-checklist items verified.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor send::` → `PASS 4`.

### S26w — Wire the request editor
**Goal:** the running app opens request tabs and sends real requests.
**Depends on:** S26b, S22e. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_request_editor.rs`.

```
Append `request_editor::init(cx)` to pustman::init and add the dependency (HttpClient built on
ReqwestTransport, using gpui_tokio). Test (tests/wire_request_editor.rs):
wire_request_editor_new_request_opens_tab.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_request_editor` → `PASS 1`.
- `cargo build -p pustman` → exit 0.
- E2E: `cargo run -p pustman`, then New Request, `GET https://example.com`, then `Ctrl-Enter`. The status line shows 200 and the time.

### S27a — KeyValueTable and the Params tab with path variables
**Goal:** the Params table and the URL query stay in sync both ways with no feedback loop, and
`:name` path segments get rows.
**Depends on:** S26b. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/key_value_table.rs`, `crates/request_editor/src/params.rs`,
`crates/request_editor/src/params_tests.rs`, `crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-HTTP-02, crates/ui Table. src/key_value_table.rs: reusable KeyValueTable (key,
value, description, enabled; add/remove rows; keyboard navigable). src/params.rs: Params tab,
two-way sync with url_bar via UrlChanged/set_url (guard against loops); path-variable rows from
`:name`. Add the request tab strip (Params now; later tabs append) to request_editor.rs. Tests
(params_tests.rs): editing_url_updates_params_table, editing_params_updates_url,
path_variable_rows_follow_url.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor params::` → `PASS 3`.

### S27b — Headers tab with auto headers
**Goal:** the Headers tab edits headers, autocompletes header names, and shows auto headers greyed
with an "auto" badge.
**Depends on:** S27a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/headers.rs`, `crates/request_editor/src/headers_tests.rs`,
`crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-HTTP-03, crates/http_engine (ResolvedRequest.auto_headers, prepare).
src/headers.rs: Headers tab on KeyValueTable; header-name autocomplete from a static common-headers
list; auto headers from a dry prepare shown read-only. Tests (headers_tests.rs):
auto_headers_listed_read_only.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor headers::` → `PASS 1`.

### S27c — cURL paste and Copy as cURL
**Goal:** pasting text that starts with `curl ` into the URL bar imports the request (undoable,
parse errors inline). `request::CopyAsCurl` copies the request as a cURL command.
**Depends on:** S27b, S10. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/curl_paste.rs`, `crates/request_editor/src/curl_paste_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only).

```
Read docs/PRD.md REQ-HTTP-13, crates/api_model/src/curl.rs. src/curl_paste.rs: intercept paste in the
URL bar → parse_curl → replace the request as one undoable change; error → inline. CopyAsCurl →
to_curl to the clipboard. Tests (curl_paste_tests.rs): pasting_curl_imports_request,
copy_as_curl_writes_clipboard.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor curl_paste::` → `PASS 2`.

### S28a — Body tab
**Goal:** every M1 body mode is editable (none, form-data with a file picker, urlencoded, raw with
a language, binary). File paths inside the workspace are stored relative to its root.
**Depends on:** S27c. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/body_tab.rs`, `crates/request_editor/src/body_tab_tests.rs`,
`crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-HTTP-04/05, crates/api_model Body. src/body_tab.rs: mode switcher that keeps each
mode's data when switching; form-data rows with file fields (file dialog); raw TextArea with a
language picker; binary file picker; workspace-relative paths. Tests (body_tab_tests.rs):
body_mode_switch_preserves_other_modes_data, file_path_saved_relative_to_workspace.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor body_tab::` → `PASS 2`.

### S28b — Auth tab
**Goal:** the Auth tab edits Inherit, None, Basic, Bearer and API key, shows "Inherited from
<collection/folder>", and hints at a secret variable when a literal token is typed.
**Depends on:** S28a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/auth_tab.rs`, `crates/request_editor/src/auth_tab_tests.rs`,
`crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-HTTP-06, REQ-MODEL-01/03, REQ-VAR-SHARE-03, crates/api_model/src/auth.rs
(effective_auth). src/auth_tab.rs: the auth types; inherited source label; a literal token in a
shared field shows the REQ-VAR-SHARE-03 hint. Never log values. Tests (auth_tab_tests.rs):
auth_tab_shows_inherited_source, literal_token_shows_secret_hint.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor auth_tab::` → `PASS 2`.

### S28c — Settings tab and TLS verification badge
**Goal:** per-request settings are editable, each with a "use global default" state. Turning TLS
verification off shows a persistent warning badge beside Send. A TLS failure offers "Disable
verification for this request", which sets the override and re-sends.
**Depends on:** S28b. **Deploy delta:** none. **High-stakes (TLS).**
**Edits:** `crates/request_editor/src/settings_tab.rs`, `crates/request_editor/src/settings_tab_tests.rs`,
`crates/request_editor/src/request_editor.rs`, `crates/request_editor/src/send.rs`.

```
Read docs/PRD.md REQ-HTTP-09/10, the HTTP AC about self-signed TLS, crates/http_engine
RequestSettings. src/settings_tab.rs: each RequestSettings field with "use global default". The badge
shows next to Send whenever verify_tls is off for this request. On TransportError::Tls the inline
error offers "Disable verification for this request" → set the override, re-send. TLS stays on by
default; never disable it globally from here. Tests (settings_tab_tests.rs):
tls_error_offers_disable_and_resend, tls_badge_visible_while_verify_disabled.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor settings_tab::` → `PASS 2`.
- E2E: run `openssl s_server` on a self-signed cert at `https://localhost:8443`. The first send shows the TLS error. "Disable verification" makes it succeed, and the badge stays visible.

### S29a — `response_viewer` crate: status line, headers, Raw body
**Goal:** a `ResponseView` shows the status line (status colored from the theme, reason, time,
size), a headers table that keeps duplicates in order, and the body in Raw mode as a virtualized
list of lines.
**Depends on:** S25, S16b. **Deploy delta:** none.
**Edits:** `crates/response_viewer/Cargo.toml`, `crates/response_viewer/src/response_viewer.rs`,
`crates/response_viewer/src/status_line.rs`, `crates/response_viewer/src/status_line_tests.rs`,
`crates/response_viewer/src/headers_view.rs`, `crates/response_viewer/src/headers_view_tests.rs`,
`crates/response_viewer/src/body_view.rs`, `crates/response_viewer/src/body_view_tests.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-RESP-01/02/05 (headers part), crates/http_engine Response.
src/response_viewer.rs: ResponseView entity with `set_response(...)`, tabs Body/Headers (later tabs
append), `pub fn init`. src/status_line.rs, src/headers_view.rs, src/body_view.rs (Raw mode only;
Pretty is S29b). Do not touch request_editor or crates/pustman. Tests: status_line_tests.rs:
status_class_colors_from_theme; headers_view_tests.rs: duplicate_headers_kept_in_order;
body_view_tests.rs: raw_mode_shows_exact_bytes.
```

**Verify:**
- `scripts/pustman-v1-test.sh response_viewer status_line::` → `PASS 1`.
- `scripts/pustman-v1-test.sh response_viewer headers_view::` → `PASS 1`.
- `scripts/pustman-v1-test.sh response_viewer body_view::` → `PASS 1`.

### S29b — Pretty body mode
**Goal:** the Pretty mode formats JSON and indents XML and HTML. The formatter is auto-detected from
Content-Type and can be overridden by hand.
**Depends on:** S29a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/pretty.rs`, `crates/response_viewer/src/pretty_tests.rs`,
`crates/response_viewer/src/body_view.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read docs/PRD.md REQ-RESP-02. src/pretty.rs: formatters (serde_json pretty; simple XML/HTML indent)
and Content-Type detection; body_view gains a Pretty/Raw switch and a formatter override. Tests
(pretty_tests.rs): json_body_pretty_printed, content_type_override_switches_formatter.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer pretty::` → `PASS 2`.

### S29c — Error panel
**Goal:** a `TransportError` renders a panel with its type, message and suggested fix. The "Open in
Console" link stays hidden until the console exists; it is never stubbed.
**Depends on:** S29a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/error_panel.rs`, `crates/response_viewer/src/error_panel_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`.

```
Read docs/PRD.md REQ-RESP-08, §8.6. src/error_panel.rs: panel from TransportError
user_message/suggested_fix; ResponseView shows it instead of the body on error; the "Open in Console"
link dispatches console::Focus only when that action is registered, otherwise it is hidden. Tests
(error_panel_tests.rs): transport_error_renders_panel_with_fix.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer error_panel::` → `PASS 1`.

### S29e — Embed the response view under the request editor
**Goal:** the request editor shows a `ResponseView` below it in a resizable split. The status line
moves from the editor into the viewer.
**Depends on:** S29c, S28c. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/response_split.rs`, `crates/request_editor/src/response_split_tests.rs`,
`crates/request_editor/src/request_editor.rs`, `crates/request_editor/src/send.rs`,
`crates/request_editor/Cargo.toml` (append deps).

```
Read crates/request_editor (request_editor.rs, send.rs), crates/response_viewer. src/response_split.rs:
resizable vertical split; send results go to ResponseView::set_response; remove the editor's own
status line (S26b tests must stay green — if one asserts on the old status line, stop and ask the
driver). Tests (response_split_tests.rs): send_result_appears_in_embedded_response_view.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor response_split::` → `PASS 1`.
- `scripts/pustman-v1-test.sh request_editor send::` → `PASS 4`.

### S29w — Wire the response viewer
**Goal:** the running app shows responses and the error panel.
**Depends on:** S29c, S26w. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_response_viewer.rs`.

```
Append `response_viewer::init(cx)` to pustman::init before request_editor, and add the dependency.
Test (tests/wire_response_viewer.rs): wire_response_viewer_init_registers_actions.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_response_viewer` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S30a — Hex and safe Preview views
**Goal:** a Hex view (offset | hex | ascii, virtualized) and a Preview view: images render from
bytes, and HTML shows as sanitized text with no script execution and no network fetch.
**Depends on:** S29b. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/hex_view.rs`, `crates/response_viewer/src/hex_view_tests.rs`,
`crates/response_viewer/src/preview.rs`, `crates/response_viewer/src/preview_tests.rs`,
`crates/response_viewer/src/body_view.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read docs/PRD.md REQ-RESP-02/03. src/hex_view.rs (virtualized rows). src/preview.rs: GPUI img from
bytes; HTML as sanitized plain text with a note. Add both to body_view's mode switch. Tests:
hex_view_tests.rs: hex_view_formats_offsets; preview_tests.rs: html_preview_does_not_execute_or_fetch.
```

**Verify:**
- `scripts/pustman-v1-test.sh response_viewer hex_view::` → `PASS 1`.
- `scripts/pustman-v1-test.sh response_viewer preview::` → `PASS 1`.

### S30b — Find in body
**Goal:** `Ctrl-F` opens a find bar with regex and case options, next and previous, and a match count.
**Depends on:** S30a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/find.rs`, `crates/response_viewer/src/find_tests.rs`,
`crates/response_viewer/src/body_view.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read docs/PRD.md REQ-RESP-05. src/find.rs: find bar (regex, case toggle, next/prev, count); matches
highlighted in body_view; regex compile errors shown inline. Tests (find_tests.rs):
find_regex_counts_matches, find_case_sensitive_toggle.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer find::` → `PASS 2`.

### S30c — Large-body fallback
**Goal:** a body over `response_pretty_limit_mb` shows Raw with a banner that offers "open temp file
location".
**Depends on:** S30b. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/large_body.rs`, `crates/response_viewer/src/large_body_tests.rs`,
`crates/response_viewer/src/body_view.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read docs/PRD.md REQ-RESP-06, crates/http_engine ResponseBody::Spilled. src/large_body.rs: limit check
from settings; banner; reveal the spilled temp file. Tests (large_body_tests.rs):
large_body_falls_back_to_raw_with_banner.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer large_body::` → `PASS 1`.

### S30d — Save and copy the body; copy a header
**Goal:** `response::SaveBody` writes the body to a chosen file on a background task (an error shows
a toast). `CopyBody` copies the body, and the header context menu copies a single header.
**Depends on:** S29c. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/save_body.rs`, `crates/response_viewer/src/save_body_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`, `crates/response_viewer/src/headers_view.rs`.

```
Read docs/PRD.md REQ-RESP-07. src/save_body.rs: SaveBody (file dialog, background write, toast on
error), CopyBody; a "Copy" item in headers_view's context menu. Tests (save_body_tests.rs):
save_body_writes_file.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer save_body::` → `PASS 1`.

### S30e — Cookies tab
**Goal:** a Cookies tab lists the response's `Set-Cookie` headers, parsed, with every attribute.
**Depends on:** S30d. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/cookies_tab.rs`, `crates/response_viewer/src/cookies_tab_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`.

```
Read docs/PRD.md REQ-RESP-02 (cookies), REQ-HTTP-08. src/cookies_tab.rs: parse Set-Cookie (name, value,
domain, path, expires, max-age, secure, httponly, samesite) into a table; add the tab to
response_viewer.rs. Tests (cookies_tab_tests.rs): cookies_tab_lists_attributes.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer cookies_tab::` → `PASS 1`.

### S31a — Timeline tab: phases, network info, redirect chain
**Goal:** the Timeline tab draws bars only for measured phases and notes the phases this transport
cannot measure. It also shows network and TLS info and every redirect hop.
**Depends on:** S30e. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/timeline.rs`, `crates/response_viewer/src/timeline_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`.

```
Read docs/PRD.md §8.2 (REQ-NET-01..04), docs/decisions/ADR-0003-timing-phases.md, crates/http_engine
Response/Timings/NetworkInfo/RedirectHop. src/timeline.rs: bars only for phases that are Some; the
ADR-0003 note; network info section; redirect chain list (method, URL, status, Location, timing;
expandable headers). Add the tab. Tests (timeline_tests.rs): absent_phases_not_rendered,
redirect_chain_lists_every_hop.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer timeline::` → `PASS 2`.

### S31b — Request as sent
**Goal:** the Timeline's "Request as sent" section shows the final URL, the method, every header
(auto, auth and cookie headers included) and a body preview, with secrets and `Authorization` masked.
**Depends on:** S31a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/request_as_sent.rs`, `crates/response_viewer/src/request_as_sent_tests.rs`,
`crates/response_viewer/src/timeline.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read crates/api_model/src/redact.rs, crates/http_engine ResolvedRequest/secrets_used. src/request_as_sent.rs:
render through api_model::redact with the send's secrets. Never log values. Tests
(request_as_sent_tests.rs): request_as_sent_includes_auto_and_cookie_headers,
request_as_sent_masks_secrets_and_authorization.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer request_as_sent::` → `PASS 2`.

### S36a — Manage Cookies dialog: list and delete
**Goal:** a modal lists the workspace cookie jar grouped by domain and can delete a cookie or clear a
domain. It opens from the Cookies tab and from `cookies::Manage`.
**Depends on:** S30e, S15b, S22d. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/cookie_dialog.rs`, `crates/response_viewer/src/cookie_dialog_tests.rs`,
`crates/response_viewer/src/cookies_tab.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod`/init lines only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-HTTP-08, crates/http_engine/src/{cookies,cookie_persistence}.rs,
crates/workspace ModalLayer. src/cookie_dialog.rs: modal grouped by domain; delete one / clear domain;
persisted through the CookieJar API on a background task; errors inline. A "Manage cookies" button
in cookies_tab.rs; register cookies::Manage. Never log cookie values. Tests (cookie_dialog_tests.rs):
dialog_lists_cookies_by_domain, delete_cookie_removes_row.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer cookie_dialog::` → `PASS 2`.

### S36b — Manage Cookies dialog: add and edit
**Goal:** the dialog's add and edit form validates input and persists the change.
**Depends on:** S36a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/cookie_form.rs`, `crates/response_viewer/src/cookie_form_tests.rs`,
`crates/response_viewer/src/cookie_dialog.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Read crates/response_viewer/src/cookie_dialog.rs. src/cookie_form.rs: add/edit form (name, value,
domain, path, expiry, flags) with validation; save through CookieJar on a background task. Tests
(cookie_form_tests.rs): add_cookie_persists, edit_cookie_updates_value.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer cookie_form::` → `PASS 2`.

### S32a — `console` crate: ConsoleStore ring buffer
**Goal:** a `ConsoleStore` global keeps entries in a ring buffer capped by `console_max_entries`
(5,000 by default). It is memory only and never written to disk, and it can be paused.
**Depends on:** S25, S3, S11. **Deploy delta:** none.
**Edits:** `crates/console/Cargo.toml`, `crates/console/src/console.rs`, `crates/console/src/store.rs`,
`crates/console/src/store_tests.rs`.

```
Read docs/PRD.md §8.3 (REQ-CON-01..04, 06), REQ-NFR-04. src/store.rs: ConsoleEntry (summary line:
method, URL, status, time, size; level; protocol; raw request/response text) and the ConsoleStore
Global (ring buffer, cap from settings, pause flag, clear). src/console.rs: `pub fn init`. Do not touch
crates/pustman or http_engine. Tests (store_tests.rs): cap_drops_oldest_entries, pause_stops_new_entries.
```

**Verify:** `scripts/pustman-v1-test.sh console store::` → `PASS 2`.

### S32b — Console feed from sends and warnings
**Goal:** every send adds an entry whose raw view shows the resolved request and response exactly as
on the wire (HTTP/1.1 form; HTTP/2 as pseudo-headers), redacted. Warnings (unresolved variables, TLS
disabled, missing body file, script not executed) and cancelled sends land as entries too.
**Depends on:** S32a, S16b, S15c. **Deploy delta:** none.
**Edits:** `crates/console/src/feed.rs`, `crates/console/src/feed_tests.rs`,
`crates/console/src/console.rs` (append `mod`/init lines only), `crates/console/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-CON-01/02, REQ-HTTP-12 (Cancelled entry), crates/http_engine (SendEvents,
Warning), crates/api_model/src/redact.rs. src/feed.rs: subscribe to http_engine SendEvents and build
entries, redacted with the send's secrets. If http_engine has no event hook to subscribe to, do NOT
edit http_engine (outside this Edits set): record a Decision and stop to ask the driver. Tests
(feed_tests.rs): send_creates_entry_with_summary, raw_view_shows_resolved_request_exactly,
tls_disabled_warning_logged.
```

**Verify:** `scripts/pustman-v1-test.sh console feed::` → `PASS 3`.

### S32c — Redacted text export
**Goal:** `export_text(entries) -> String` exports entries as redacted text. S37 relies on it.
**Depends on:** S32a. **Deploy delta:** none.
**Edits:** `crates/console/src/export.rs`, `crates/console/src/export_tests.rs`,
`crates/console/src/console.rs` (append `mod` only).

```
Read docs/PRD.md REQ-CON-05/06, crates/api_model/src/redact.rs. src/export.rs: `pub fn
export_text(entries: &[ConsoleEntry], secrets: &SecretValues) -> String`, redacting header rules
and secret values again even though entries are already redacted (defense in depth). Tests
(export_tests.rs): export_text_contains_no_secret.
```

**Verify:** `scripts/pustman-v1-test.sh console export::` → `PASS 1`.

### S32d — Console panel
**Goal:** the Console registers in the bottom dock through the `Panel` trait, with level, protocol
and text filters, clear and pause. `Ctrl-Alt-C` toggles it.
**Depends on:** S32a, S22c. **Deploy delta:** none.
**Edits:** `crates/console/src/panel.rs`, `crates/console/src/panel_tests.rs`,
`crates/console/src/console.rs` (append `mod`/init lines only), `crates/console/Cargo.toml` (append deps).

```
Load rust-gpui-ui-ux. Read crates/workspace/src/panel.rs, crates/console/src/store.rs. src/panel.rs:
ConsolePanel with a virtualized entry list, expandable raw view, filters, clear, pause; registers
console::Focus and the toggle. Tests (panel_tests.rs): filters_by_level_and_text.
```

**Verify:** `scripts/pustman-v1-test.sh console panel::` → `PASS 1`.

### S32w — Wire the console
**Goal:** the running app logs every send to the Console.
**Depends on:** S32b, S32c, S32d, S22e. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_console.rs`.

```
Append `console::init(cx)` to pustman::init after workspace, and add the dependency. Test
(tests/wire_console.rs): wire_console_panel_registered_in_bottom_dock.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_console` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S33a — Variable highlighting in request fields
**Goal:** `{{var}}` in the URL, params, headers, raw body and auth fields is highlighted green when
resolved, red when unresolved, and masked when it is a secret.
**Depends on:** S29e. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/variable_highlight.rs`, `crates/request_editor/src/variable_highlight_tests.rs`,
`crates/request_editor/src/url_bar.rs`, `crates/request_editor/src/params.rs`,
`crates/request_editor/src/headers.rs`, `crates/request_editor/src/body_tab.rs`,
`crates/request_editor/src/auth_tab.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Read docs/PRD.md REQ-VAR-03, crates/variables (spans), crates/text_input (set_highlights).
src/variable_highlight.rs: compute spans for a field from the current scope stack, map them to
theme highlight styles, and refresh when the field or the scopes change. Hook it into each field
module. Tests (variable_highlight_tests.rs): resolved_unresolved_secret_spans_highlighted.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor variable_highlight::` → `PASS 1`.

### S33b — Variable autocomplete
**Goal:** typing `{{` opens a keyboard-navigable popover with the variable names of every scope in
effect, and choosing one inserts it.
**Depends on:** S33a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/variable_autocomplete.rs`, `crates/request_editor/src/variable_autocomplete_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod` only), `crates/text_input/src/completion.rs`,
`crates/text_input/src/text_input.rs` (append `mod` only).

```
Read docs/PRD.md REQ-VAR-04, crates/text_input. text_input/src/completion.rs: the minimum hook for a
completion popover (trigger text, anchor position, insert) — nothing else in text_input changes.
request_editor/src/variable_autocomplete.rs: names across scopes, keyboard navigation, insertion.
Tests (variable_autocomplete_tests.rs): autocomplete_lists_names_after_braces,
autocomplete_inserts_selected_name.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor variable_autocomplete::` → `PASS 2`.
- `scripts/pustman-v1-test.sh text_input input::` → `PASS 3`.

### S33c — Variable hover
**Goal:** hovering a `{{var}}` shows its value and the scope it came from. Secrets are masked, with
a reveal toggle.
**Depends on:** S33b. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/variable_hover.rs`, `crates/request_editor/src/variable_hover_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Read docs/PRD.md REQ-VAR-07, crates/request_editor/src/variable_highlight.rs. src/variable_hover.rs:
a tooltip over highlighted spans; the reveal toggle reads the keychain on a background task. Never
log values. Tests (variable_hover_tests.rs): hover_masks_secret_value.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor variable_hover::` → `PASS 1`.

### S33d — Pre-send unresolved-variable warning
**Goal:** when `prepare()` reports `UnresolvedVariables`, a non-blocking warning bar lists the names
and offers "Send anyway". The warning is also logged to the Console.
**Depends on:** S29e. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/unresolved_warning.rs`, `crates/request_editor/src/unresolved_warning_tests.rs`,
`crates/request_editor/src/send.rs`, `crates/request_editor/src/request_editor.rs`.

```
Read docs/PRD.md REQ-VAR-04, crates/http_engine Warning, crates/request_editor/src/send.rs.
src/unresolved_warning.rs: warning bar above the URL bar; "Send anyway" proceeds; the warning
travels with the send's events, which feed the Console. Tests (unresolved_warning_tests.rs):
unresolved_warning_lists_names_and_allows_send.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor unresolved_warning::` → `PASS 1`.

### S34a — `environment_picker` crate: active environment selector
**Goal:** a title-bar picker lists the environments plus "No environment". The selection is stored
per workspace in local state.
**Depends on:** S25, S4, S7, S6. **Deploy delta:** none.
**Edits:** `crates/environment_picker/Cargo.toml`, `crates/environment_picker/src/environment_picker.rs`,
`crates/environment_picker/src/selector.rs`, `crates/environment_picker/src/selector_tests.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-VAR-08, crates/storage kv, crates/command_palette picker
(reuse it if generic, otherwise a small dropdown). src/selector.rs: picker, selection persisted in
storage kv per workspace on a background task; `pub fn init`. Do not touch crates/pustman. Tests
(selector_tests.rs): selecting_environment_persists_per_workspace,
no_environment_option_clears_selection.
```

**Verify:** `scripts/pustman-v1-test.sh environment_picker selector::` → `PASS 2`.

### S34b — Environment quick-look
**Goal:** a quick-look popover lists the active environment and globals (key, shared value, local
value), with secrets masked and unset secrets labelled "unset".
**Depends on:** S34a. **Deploy delta:** none.
**Edits:** `crates/environment_picker/src/quick_look.rs`, `crates/environment_picker/src/quick_look_tests.rs`,
`crates/environment_picker/src/environment_picker.rs` (append `mod`/init lines only).

```
Read docs/PRD.md REQ-VAR-SHARE-01/02, crates/secrets. src/quick_look.rs: popover; the reveal toggle
reads the keychain on a background task. Never log values. Tests (quick_look_tests.rs):
quick_look_masks_secrets, unset_secret_labelled_unset.
```

**Verify:** `scripts/pustman-v1-test.sh environment_picker quick_look::` → `PASS 2`.

### S34w — Wire the environment picker
**Goal:** the running app shows the environment picker in the title bar.
**Depends on:** S34b, S22e. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_environment_picker.rs`.

```
Append `environment_picker::init(cx)` to pustman::init and add the dependency. Test
(tests/wire_environment_picker.rs): wire_environment_picker_init_registers_actions.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_environment_picker` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S35a — `history_panel` crate: list grouped by day, with search
**Goal:** a left-dock History panel lists entries grouped by day in a virtualized list with a search box.
**Depends on:** S25, S16b, S22c, S6. **Deploy delta:** none.
**Edits:** `crates/history_panel/Cargo.toml`, `crates/history_panel/src/history_panel.rs`,
`crates/history_panel/src/history_list.rs`, `crates/history_panel/src/history_list_tests.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md §8.5 (REQ-HIST-01..04), crates/storage history repo,
crates/workspace/src/panel.rs. src/history_list.rs: Panel in the left dock; DB access on background
tasks; errors → toast. "Save to collection" is M2 (hidden, not stubbed). Do not touch crates/pustman.
Tests (history_list_tests.rs): entries_grouped_by_day, search_filters_entries.
```

**Verify:** `scripts/pustman-v1-test.sh history_panel history_list::` → `PASS 2`.

### S35b — Open a history entry
**Goal:** opening an entry opens a request tab with the stored pre-resolution request and a read-only
response summary.
**Depends on:** S35a, S26b. **Deploy delta:** none.
**Edits:** `crates/history_panel/src/open_entry.rs`, `crates/history_panel/src/open_entry_tests.rs`,
`crates/history_panel/src/history_panel.rs` (append `mod`/init lines only), `crates/history_panel/Cargo.toml` (append deps).

```
Read crates/request_editor (opening a tab from an HttpRequest). src/open_entry.rs: open → new
RequestEditor tab; the response summary is read-only. Tests (open_entry_tests.rs):
open_entry_restores_unresolved_request.
```

**Verify:** `scripts/pustman-v1-test.sh history_panel open_entry::` → `PASS 1`.

### S35c — Delete and clear history
**Goal:** a single entry can be deleted, and "Clear all" (behind a confirm modal) removes every entry.
**Depends on:** S35a. **Deploy delta:** none.
**Edits:** `crates/history_panel/src/delete_entries.rs`, `crates/history_panel/src/delete_entries_tests.rs`,
`crates/history_panel/src/history_panel.rs` (append `mod`/init lines only).

```
Read crates/storage history repo, crates/workspace ModalLayer. src/delete_entries.rs: delete one,
clear all with confirm; background tasks; errors → toast. Tests (delete_entries_tests.rs):
delete_and_clear_all_remove_entries.
```

**Verify:** `scripts/pustman-v1-test.sh history_panel delete_entries::` → `PASS 1`.

### S35w — Wire the history panel
**Goal:** the running app shows the History panel in the left dock.
**Depends on:** S35b, S35c, S22e. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_history_panel.rs`.

```
Append `history_panel::init(cx)` to pustman::init and add the dependency. Test
(tests/wire_history_panel.rs): wire_history_panel_registered_in_left_dock.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_history_panel` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S37 — Redaction NFR test (integration)
**Goal:** one test proves that no secret value reaches workspace files, the SQLite file bytes, or
Console export text.
**Depends on:** S5, S16b, S15c, S32c. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/integration_tests/**`.

```
Read docs/PRD.md REQ-NFR-04, REQ-WSP-06, REQ-HIST-03, REQ-CON-05/06, plan §9. Create
crates/integration_tests (publish = false, dev-only; deps on the crates under test with their
test-support features). tests/redaction.rs (#[gpui::test]): fixture workspace in a tempdir with a
secret variable (local value "S3cr3t-Value-123"), a request using it in URL query, a header,
Basic auth password, Bearer token and JSON body; send through http_engine with FakeTransport
echoing the request and setting a cookie containing the secret; then assert the secret string
appears in NONE of: every file under the workspace dir (after save_item), the SQLite file bytes
(after history write + cookie persist — cookies are local state but the cookie VALUE here equals
the secret so it must be absent or the test must document why the jar is exempt: the jar is
local state per REQ-WSP-06 and NOT redacted; assert instead that it is absent from history and
workspace files, and record that Decision), console::export_text output. Name the test
`no_secret_in_workspace_files_sqlite_history_or_console_export`, plus
`secret_variable_file_has_key_kind_description_only`.
```

**Verify:** `scripts/pustman-v1-test.sh integration_tests secret` → `PASS 2`. The filter `secret`
matches both test names. The original filter `redaction` matched neither, because the runner's
filter matches the bare function names of integration tests.

### S38 — M1 SIGN-OFF (HUMAN)
**Goal:** the M1 exit criteria are proven: the PRD HTTP ACs pass, the redaction test passes, and the manual run-through is done.
**Depends on:** S26w, S28c, S29w, S29e, S30c, S31b, S32w, S33c, S33d, S34w, S35w, S36b, S37. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux + macOS: the four PRD §5.1 HTTP ACs by hand (echo server: send `POST {{base}}/users`
with the JSON body `{"name":"{{name}}"}`; the Console raw view shows the resolved URL, the body and `Content-Type: application/json`;
a 6-hop redirect server with max 5; openssl s_server self-signed; cancel immediately after send — Console
shows Cancelled, no retry); variables highlighting/autocomplete; history open/restore; cookies
dialog; cURL paste/copy; Timeline on a real HTTPS host. Confirm `scripts/pustman-v1-test.sh
integration_tests secret` → PASS 2 on the merge SHA. Record M1 GO/NO-GO; then /promote →
release/pustman-v1-3.
```

**Verify:** the log records `M1 GO` and the four ACs by name. CI is green at the merge SHA.

---

# Phase 5 — Workspaces + sharing (M2) [GPUI]

### S39a — `collections_panel` crate: virtualized tree
**Goal:** the left dock shows the open workspace's collections, folders and requests as a
virtualized tree that follows the `order` arrays, with method badges and expand/collapse.
**Depends on:** S38. **Deploy delta:** none.
**Edits:** `crates/collections_panel/Cargo.toml`, `crates/collections_panel/src/collections_panel.rs`,
`crates/collections_panel/src/tree.rs`, `crates/collections_panel/src/tree_tests.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-UI-01, REQ-NFR-02 (10,000-item tree at 60 fps),
crates/workspace_store (snapshot), crates/workspace/src/panel.rs. src/tree.rs: Panel in the left
dock; rows from the WorkspaceSnapshot honoring `order`; expand/collapse; method badges; virtualized
(uniform_list); element ids from ItemId. Problems from the snapshot show an error badge (detail UI
is S42b). Keyboard navigation and opening tabs are S39b. `pub fn init` in collections_panel.rs. Do
not touch crates/pustman. Tests (tree_tests.rs): tree_follows_order_arrays,
ten_thousand_items_render_only_visible_rows.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel tree::` → `PASS 2`.

### S39b — Tree keyboard navigation and opening requests
**Goal:** arrow keys move through the tree and expand or collapse folders. Enter opens the request's
tab, one tab per item id, and opening it again focuses the existing tab.
**Depends on:** S39a. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/navigation.rs`, `crates/collections_panel/src/navigation_tests.rs`,
`crates/collections_panel/src/tree.rs`, `crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only),
`crates/collections_panel/Cargo.toml` (append deps).

```
Read crates/collections_panel/src/tree.rs, crates/request_editor (opening a tab for an item),
crates/workspace (Item, Pane). src/navigation.rs: selection model, Up/Down/Left/Right/Enter
handlers; Enter opens or focuses the RequestEditor tab keyed by ItemId. Tests (navigation_tests.rs):
keyboard_navigation_expands_and_opens, opening_twice_focuses_existing_tab.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel navigation::` → `PASS 2`.

### S39w — Wire the collections panel
**Goal:** the running app shows the collections tree in the left dock.
**Depends on:** S39b. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_collections_panel.rs`.

```
Append `collections_panel::init(cx)` to pustman::init and add the dependency. Test
(tests/wire_collections_panel.rs): wire_collections_panel_registered_in_left_dock.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_collections_panel` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S40a — Collection CRUD
**Goal:** a context menu and actions create, rename (inline), duplicate and delete (behind a confirm
modal) collections, folders and requests. Every change writes through `workspace_store`.
**Depends on:** S39b. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/crud.rs`, `crates/collections_panel/src/crud_tests.rs`,
`crates/collections_panel/src/tree.rs`, `crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only).

```
Read docs/PRD.md REQ-WSP-01..03, REQ-UI-03, crates/workspace_store save_item, crates/workspace
ModalLayer. src/crud.rs: context menu (ui ContextMenu) + actions new collection/folder/request,
rename (keeps the id, updates the slug), duplicate, delete with confirm (removes the file(s)). File
IO on background tasks; failures → toast with path and reason. Tests (crud_tests.rs):
create_request_writes_file_and_appears_in_tree, rename_keeps_id_and_updates_slug,
delete_removes_file_after_confirm.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel crud::` → `PASS 3`.

### S40b — Drag to reorder and move
**Goal:** dragging a node reorders it or moves it into another folder, and the parent's `order` array
is updated on disk.
**Depends on:** S40a. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/reorder.rs`, `crates/collections_panel/src/reorder_tests.rs`,
`crates/collections_panel/src/tree.rs`, `crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only).

```
Read crates/collections_panel (tree.rs, crud.rs), crates/workspace_store. src/reorder.rs: drag and
drop on tree rows (GPUI on_drag/on_drop) plus keyboard move actions; writes the parent's order array
via workspace_store on a background task; failure → toast. Tests (reorder_tests.rs):
reorder_updates_parent_order_array.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel reorder::` → `PASS 1`.

### S40c — Save the active request (`Ctrl-S`)
**Goal:** `workspace::Save` in the request editor writes the request to its file through
`workspace_store`. Success clears the dirty dot, and failure shows a toast with the path and reason.
**Depends on:** S38. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/save.rs`, `crates/request_editor/src/save_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only), `crates/request_editor/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-WSP-01, REQ-UI-02, crates/workspace_store save_item, crates/request_editor.
src/save.rs: Save handler registered on RequestEditor; save_item on a background task; clears dirty
on success; toast on failure. Tests (save_tests.rs): ctrl_s_saves_and_clears_dirty,
save_failure_shows_toast.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor save::` → `PASS 2`.

### S41a — Open and create workspaces
**Goal:** the user can open a folder as a workspace or create a new one, one workspace per window.
The last workspace reopens at launch and is recorded in the recent list.
**Depends on:** S38, S23b, S39a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/workspace_open.rs`, `crates/workspace/src/workspace_open_tests.rs`,
`crates/workspace/src/workspace.rs` (append `mod`/init lines only), `crates/workspace/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-WSP-04/05, REQ-COLLAB-06, crates/workspace_store open, crates/storage
recent_workspaces. src/workspace_open.rs: OpenWorkspace (folder dialog), CreateWorkspace (writes
pustman.json + layout; inside a git repo, show the REQ-COLLAB-06 note that no local files are
stored there); records the workspace in recent_workspaces; reopens the last one at launch. IO on
background tasks; errors → toast. Tests (workspace_open_tests.rs): create_workspace_writes_pustman_json,
open_workspace_loads_snapshot_and_records_recent.
```

**Verify:** `scripts/pustman-v1-test.sh workspace workspace_open::` → `PASS 2`.

### S41b — Open recent
**Goal:** an `OpenRecent` picker lists recent workspaces, most recent first.
**Depends on:** S41a, S24b. **Deploy delta:** none.
**Edits:** `crates/command_palette/src/recent.rs`, `crates/command_palette/src/recent_tests.rs`,
`crates/command_palette/src/command_palette.rs` (append `mod`/init lines only), `crates/command_palette/Cargo.toml` (append deps).

```
Read crates/workspace/src/workspace_open.rs, crates/command_palette (picker.rs, palette.rs). The
picker lives in command_palette because command_palette already depends on workspace (ModalLayer);
workspace must never depend on command_palette. src/recent.rs: OpenRecent action → Picker over
recent_workspaces, newest first; Enter opens via workspace_open. Do not touch crates/workspace or
crates/pustman (command_palette::init is already wired by S24w). Tests (recent_tests.rs):
recent_list_most_recent_first.
```

**Verify:** `scripts/pustman-v1-test.sh command_palette recent::` → `PASS 1`.

### S41c — Newer-format read-only banner
**Goal:** a workspace whose format has a newer major version opens read-only with a banner. Editing
is disabled, and save actions do nothing except show a toast.
**Depends on:** S41a. **Deploy delta:** none.
**Edits:** `crates/workspace/src/read_only.rs`, `crates/workspace/src/read_only_tests.rs`,
`crates/workspace/src/workspace.rs`.

```
Read docs/PRD.md REQ-WSP-05, crates/workspace_store (read_only snapshot flag). src/read_only.rs:
banner rendered from workspace.rs; a read_only flag items query to disable editing; Save → toast.
Tests (read_only_tests.rs): newer_major_opens_read_only_with_banner.
```

**Verify:** `scripts/pustman-v1-test.sh workspace read_only::` → `PASS 1`.

### S42a — Watcher updates the tree
**Goal:** the watcher starts when a workspace opens, and external adds, edits and deletes show up in
the tree within 1 s.
**Depends on:** S40b, S41a, S8. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/external.rs`, `crates/collections_panel/src/external_tests.rs`,
`crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only), `crates/collections_panel/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-COLLAB-01, crates/workspace_store watcher, crates/workspace/src/workspace_open.rs.
src/external.rs: start the watcher on workspace open (store the subscription/task); apply FsChange
to the snapshot; the tree re-renders from the snapshot (do not edit tree.rs). Tests (external_tests.rs;
tempdir workspace, real watcher, bounded GPUI-timer wait): external_add_appears_in_tree.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel external::` → `PASS 1`.

### S42b — Problem badges with file and line
**Goal:** a conflicted or invalid-JSON file shows an error badge on its tree item and a message that
names the file and the line.
**Depends on:** S40b. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/problems.rs`, `crates/collections_panel/src/problems_tests.rs`,
`crates/collections_panel/src/tree.rs`, `crates/collections_panel/src/collections_panel.rs` (append `mod` only).

```
Read docs/PRD.md REQ-COLLAB-02, crates/workspace_store (Problem). src/problems.rs: badge + tooltip
message "<file>:<line> <reason>" on the tree row; clicking it opens a details popover. Tests
(problems_tests.rs): conflict_file_shows_badge_with_line.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel problems::` → `PASS 1`.

### S42c — "Changed on disk" bar in the request editor
**Goal:** a clean open tab reloads silently when its file changes. A dirty tab shows a "Changed on
disk — Reload / Keep mine / Compare" bar.
**Depends on:** S40c, S42a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/disk_change.rs`, `crates/request_editor/src/disk_change_tests.rs`,
`crates/request_editor/src/request_editor.rs`, `crates/request_editor/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-COLLAB-02, crates/workspace_store watcher, crates/request_editor/src/save.rs.
src/disk_change.rs: on FsChange for the tab's item: clean → reload; dirty → bar with Reload
(discard the draft), Keep mine (next save overwrites), Compare (side-by-side field diff using
`similar`). Tests (disk_change_tests.rs): dirty_tab_shows_changed_on_disk_bar, reload_discards_draft,
keep_mine_overwrites_on_save.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor disk_change::` → `PASS 3`.

### S43a — Environment editor: shared and local values
**Goal:** an editor for environments, collection variables and globals shows key, shared value,
local value, kind (Default/Secret), enabled and description. Shared values save to the file and local
values to storage. Secret rows have no shared-value field.
**Depends on:** S38. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/environment_picker/src/env_editor.rs`, `crates/environment_picker/src/env_editor_tests.rs`,
`crates/environment_picker/src/environment_picker.rs` (append `mod`/init lines only), `crates/environment_picker/Cargo.toml` (append deps).

```
Read docs/PRD.md §6.2 (REQ-VAR-SHARE-01..03), crates/workspace_store, crates/storage local_values.
src/env_editor.rs: editor item or modal with a ui Table; Secret rows render no shared-value field;
saving writes the environment file via workspace_store and local values via storage, both on
background tasks; errors → toast. Secret values are S43b. Tests (env_editor_tests.rs):
secret_row_has_no_shared_value_field, shared_value_written_to_file_local_value_to_storage.
```

**Verify:** `scripts/pustman-v1-test.sh environment_picker env_editor::` → `PASS 2`.

### S43b — Secret values go to the keychain
**Goal:** a secret's value is set and read through `SecretStore` (the OS keychain) and never written
to the environment file or storage.
**Depends on:** S43a. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/environment_picker/src/secret_values.rs`, `crates/environment_picker/src/secret_values_tests.rs`,
`crates/environment_picker/src/env_editor.rs`, `crates/environment_picker/src/environment_picker.rs` (append `mod` only).

```
Read docs/PRD.md REQ-VAR-SHARE-02, crates/secrets (SecretStore, MemorySecretStore in tests),
crates/environment_picker/src/env_editor.rs. src/secret_values.rs: masked value field on Secret rows;
set/read via SecretStore on background tasks; never log values. Tests (secret_values_tests.rs):
secret_value_goes_to_keychain_not_file.
```

**Verify:** `scripts/pustman-v1-test.sh environment_picker secret_values::` → `PASS 1`.

### S43c — "Set secret" from the unresolved-secret error panel
**Goal:** the `UnresolvedSecret` error panel offers a "Set <name>" button that opens the editor
focused on that secret. After it is set, re-sending works.
**Depends on:** S43b. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/response_viewer/src/set_secret.rs`, `crates/response_viewer/src/set_secret_tests.rs`,
`crates/response_viewer/src/error_panel.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only),
`crates/response_viewer/Cargo.toml` (append deps).

```
Read docs/PRD.md §8.6 UnresolvedSecret, crates/response_viewer/src/error_panel.rs,
crates/environment_picker (opening the editor focused on a key). src/set_secret.rs: button per
missing name → open the editor focused on it; on save, offer Resend. Tests (set_secret_tests.rs):
unresolved_secret_panel_offers_set_and_resend.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer set_secret::` → `PASS 1`.

### S44a — `git` crate: status
**Goal:** `status(root) -> Result<Option<GitStatus { branch, dirty }>>` reports the branch and dirty
count. It returns `Ok(None)` outside a repository or when `git` is not installed.
**Depends on:** S38. **Deploy delta:** none.
**Edits:** `crates/git/Cargo.toml`, `crates/git/src/git.rs`, `crates/git/src/status.rs`,
`crates/git/src/status_tests.rs`.

```
Read docs/PRD.md REQ-COLLAB-03, AGENTS.md (smol::process, never std::process::Command). Create
crates/git (domain: no gpui by default; smol). src/status.rs: `git -C <root> status --porcelain=v2
--branch` via smol::process; parse branch (detached HEAD reported as such) and dirty count; Ok(None)
for a non-repo or missing git. Tests (status_tests.rs; tempdir repos made with smol::process git
init): status_reports_branch_and_dirty_count, non_repo_returns_none, detached_head_reported.
```

**Verify:**
- `scripts/pustman-v1-test.sh git status::` → `PASS 3`.
- `cargo tree -p git --no-default-features | grep -c gpui` → `0`.

### S44b — Status-bar branch item
**Goal:** inside a Git repository, the status bar shows the branch and dirty count. Both refresh on
watcher events and on window focus. Outside a repository the item hides.
**Depends on:** S44a, S41a. **Deploy delta:** none.
**Edits:** `crates/git/src/status_item.rs`, `crates/git/src/status_item_tests.rs`,
`crates/git/src/git.rs` (append `mod`/init lines only), `crates/git/Cargo.toml` (append deps).

```
Load rust-gpui-ui-ux. Read crates/workspace/src/status_bar.rs, crates/git/src/status.rs.
src/status_item.rs behind a `gpui` cargo feature, on by default so the test runner builds it
(the domain API stays GPUI-free under `--no-default-features`): a status-bar item
added with add_left_item; refresh on a background task. `pub fn init` behind the feature. Tests
(status_item_tests.rs): status_item_shows_branch_and_dirty_count.
```

**Verify:**
- `scripts/pustman-v1-test.sh git status_item::` → `PASS 1`.
- `cargo tree -p git --no-default-features | grep -c gpui` → `0`.

### S44w — Wire the git status item
**Goal:** the running app shows the branch in the status bar inside a repository.
**Depends on:** S44b. **Deploy delta:** none.
**Edits:** `crates/pustman/src/pustman.rs` (hotspot), `crates/pustman/Cargo.toml` (hotspot),
`crates/pustman/tests/wire_git.rs`.

```
Append `git::init(cx)` to pustman::init and depend on git with the `gpui` feature. Test
(tests/wire_git.rs): wire_git_status_item_registered.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_git` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S45a — Import a Postman collection
**Goal:** importing a Postman collection writes its items into the open workspace and shows a report.
Files over 50 MB are rejected with a message.
**Depends on:** S9, S42a, S40a. **Deploy delta:** none. **High-stakes (import).**
**Edits:** `crates/collections_panel/src/import.rs`, `crates/collections_panel/src/import_tests.rs`,
`crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only), `crates/collections_panel/Cargo.toml` (append deps).

```
Read docs/PRD.md §6.4 (REQ-IO-01/02/06), crates/api_model/src/postman.rs. src/import.rs:
import_export::Import (file dialog; size check before reading fully; read on a background task) →
import → save_item for each item → report modal (imported, preserved-but-not-executed scripts,
unsupported auth, warnings); failures → toast. Imported scripts are never executed. Tests
(import_tests.rs): import_collection_writes_files_and_shows_report, oversized_file_rejected_with_message.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel import::` → `PASS 2`.

### S45b — Import environments and globals
**Goal:** importing a Postman environment or globals file writes it into the workspace, and its
secret values go to the keychain.
**Depends on:** S45a. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/collections_panel/src/import_environment.rs`, `crates/collections_panel/src/import_environment_tests.rs`,
`crates/collections_panel/src/collections_panel.rs` (append `mod`/init lines only), `crates/collections_panel/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-IO-03, crates/api_model/src/postman.rs (SecretValuesToStore), crates/secrets.
src/import_environment.rs: reuse import.rs's dialog/size check; write the environment file; put
SecretValuesToStore into SecretStore on a background task; never log values. Tests
(import_environment_tests.rs): import_environment_secrets_go_to_keychain.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel import_environment::` → `PASS 1`.

### S45c — Export a collection
**Goal:** exporting a collection from its context menu writes Postman v2.1 and lists the items it skipped.
**Depends on:** S40b. **Deploy delta:** none.
**Edits:** `crates/collections_panel/src/export.rs`, `crates/collections_panel/src/export_tests.rs`,
`crates/collections_panel/src/crud.rs`, `crates/collections_panel/src/collections_panel.rs` (append `mod` only).

```
Read docs/PRD.md REQ-IO-06, crates/api_model/src/postman.rs (export), crates/collections_panel/src/crud.rs.
src/export.rs: context-menu entry (added in crud.rs) → file dialog → write on a background task →
a warnings list of skipped items; failure → toast. Tests (export_tests.rs):
export_writes_v21_and_lists_skipped.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel export::` → `PASS 1`.

### S46 — Quick open (`Ctrl/Cmd-P`)
**Goal:** a fuzzy finder over every request in the workspace that opens the chosen one.
**Depends on:** S41a, S24a. **Deploy delta:** none.
**Edits:** `crates/command_palette/src/quick_open.rs`, `crates/command_palette/src/quick_open_tests.rs`,
`crates/command_palette/src/command_palette.rs` (append `mod`/init lines only), `crates/command_palette/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-UI-05, crates/command_palette. src/quick_open.rs: a QuickOpen picker (reusing
the picker) listing requests from the open workspace snapshot as "Collection / Folder / Name" with
method badge; Enter opens/focuses the tab. Bound by quick_open::Toggle (already in keymaps). Tests
(quick_open_tests.rs): quick_open_lists_all_requests_with_paths, fuzzy_matches_on_path_segments,
enter_opens_request_tab.
```

**Verify:** `scripts/pustman-v1-test.sh command_palette quick_open::` → `PASS 3`.

### S47 — Sharing AC: two clones of one Git repository
**Goal:** the PRD §6.3 sharing AC passes as an automated test using two clones of a local bare repository.
**Depends on:** S37, S43b, S44a. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/integration_tests/tests/sharing.rs`, `crates/integration_tests/Cargo.toml` (append deps).

```
Read docs/PRD.md §6.3 AC (sharing), crates/workspace_store (watcher), crates/secrets
(MemorySecretStore), crates/http_engine (pipeline), crates/git. tests/sharing.rs: create a bare
repo + clones A and B in tempdirs (smol::process git; set user.name/email locally in the repos);
in A: save a request using {{api_key}} and an environment with secret `api_key` (value set in A's
MemorySecretStore); commit + push. In B: open the workspace snapshot and start the watcher; `git
pull`; assert within 1 s (GPUI test clock / bounded wait) an ItemAdded for the request; assert B's
environment shows api_key with kind Secret and no value; assert preparing a send in B fails with
UnresolvedSecret{["api_key"]}; assert `git -C A show HEAD` output does not contain the secret
value. Test name: sharing_ac_two_clones. Mark it skipped (with an explicit message, not silently)
if `git` is not installed. Do not edit tests/redaction.rs.
```

**Verify:** `scripts/pustman-v1-test.sh integration_tests sharing_ac_two_clones` → `PASS 1`.

### S48 — M2 SIGN-OFF (HUMAN)
**Goal:** the M2 exit criteria are proven by two people (or two clones) on real machines.
**Depends on:** S39w, S40c, S41b, S41c, S42b, S42c, S43c, S44w, S45b, S45c, S46, S47. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux + macOS with two clones of one GitHub repo: A adds a request + secret
api_key, commits, pushes; B (Pustman open) pulls → request appears within 1 s, api_key unset,
send prompts to set it; `git show` has no secret. Also: import a real Postman v2.1 export; export
it back and re-import in Postman; branch switch with a conflict file shows the badge; status bar
branch/dirty. Record M2 GO/NO-GO; then /promote → release/pustman-v1-4. M3–M5 are already
expanded (S49+); decide whether to expand STUB-M6 now.
```

**Verify:** the log records `M2 GO`. CI is green at the merge SHA.

---

# Phase 6 — Protocol foundation (shared by M3, M4 and M5)

Expanded on 2026-11-21, before M2 was built (progress stood at S38), on the user's "go". Every prompt below
that names an M2-era file says "grep the current code first": M2 may have changed a signature since
this text was written, and the code wins over this file. A worker that finds this file wrong reports a
`Decision:` line, and the driver corrects the step before dispatching the next one that depends on it.

The foundation steps depend on S48, and no M3, M4 or M5 step starts before they pass. After the
foundation, the three milestones interleave rather than run independently: M4 needs S52b (M3's
network model), and M5 needs S52b and S59b (M3's GraphQL errors panel, reused by S69a). Each has its
own sign-off, which promotes to the next free release tag. The cap on unpromoted steps can force an
earlier promotion, so the tag numbers are not fixed. A foundation step belongs to the first tag that
ships it (D11).

Decisions made in this expansion (they apply to every step below):

- **Dependencies are declared once, in S49.** No later step edits the root `Cargo.toml`, the same
  rule S1 set for M0–M2. A later step that needs another crate stops and reports, and the driver
  adds a dependency step.
- **No code generation.** gRPC messages are dynamic (`prost-reflect` over descriptors compiled in
  process by `protox`), and no `build.rs` or `protoc` is used anywhere. Test servers use the same
  dynamic codec, so no generated stubs exist in the repository.
- **OAuth 2.0 runs on our own HTTP engine.** Token, refresh and code-exchange requests are form POSTs sent
  through `http_engine` so they honour the proxy, certificates, Console and redaction. A
  third-party OAuth client would bypass all four. PKCE uses `sha2`, `base64` and `rand`, all declared.
- **Protocol clients are domain crates.** `websocket_client` (WebSocket and Socket.IO) and `grpc_client`
  have no GPUI views, and their editors live in `request_editor`. Socket.IO is a module of
  `websocket_client`, not a crate of its own: the PRD §9 crate list names only `websocket_client`, and
  Socket.IO runs over it.
- **File suffixes:** `.graphql.json`, `.websocket.json` (PRD §6.1), `.socketio.json` and `.grpc.json`
  (PRD §6.1).
- **Deferred to STUB-M7:**
  - the P1/P2 items in these sections: REQ-GQL-02/03/04, REQ-SSE-02/03, REQ-WS-06, REQ-GRPC-07 and
    Socket.IO v2/v3;
  - draft autosave and restore-after-restart for non-HTTP tabs. Protocol requests reopen from the
    tree.

### S49 — Declare the M3–M5 dependencies
**Goal:** every third-party crate M3, M4 and M5 need is declared once, in the root `Cargo.toml`. They
resolve on our toolchain, their licences allow us to ship them, and they share one rustls crypto provider.
**Depends on:** S48. **Deploy delta:** none.
**Edits:**
- `Cargo.toml` (root: `[workspace.dependencies]` only);
- `crates/websocket_client/Cargo.toml`, `crates/websocket_client/src/websocket_client.rs`,
  `crates/websocket_client/src/tls_provider.rs`, `crates/websocket_client/src/tls_provider_tests.rs`;
- `crates/grpc_client/Cargo.toml`, `crates/grpc_client/src/grpc_client.rs`,
  `crates/grpc_client/src/tls_provider.rs`, `crates/grpc_client/src/tls_provider_tests.rs`;
- `crates/http_engine/Cargo.toml` (append deps);
- `docs/ARCHITECTURE.md` (append rows).

```
Read AGENTS.md (Licensing, Toolchain, "Never re-implement what already exists"), docs/PRD.md §9
(candidate libraries), the root Cargo.toml. Resolve EVERY crate below with Context7 (resolve-library-id,
then query-docs) before writing its version: no version from memory. Pick the newest release that
builds on our pinned toolchain and uses the same reqwest/rustls/tokio majors we already declare.

Declare in [workspace.dependencies], grouped under a "# M3–M5 protocols" comment:
- auth: a Digest-auth crate if Context7 shows a maintained one, else md-5 (sha2 covers SHA-256);
  sha2; hmac; aws-sigv4 (+ aws-credential-types if its signing API needs it); jsonwebtoken;
  p12-keystore (PFX → PEM, because reqwest's rustls backend takes PEM identities);
- proxy: the reqwest `socks` feature, added to our existing reqwest line;
- WebSocket: tokio-tungstenite with a rustls feature that matches reqwest's;
- Socket.IO: rust_socketio (async feature), and socketioxide as a test-only server. Record in a
  Decision line which Engine.IO/Socket.IO versions rust_socketio speaks (PRD REQ-SIO-01);
- gRPC: tonic (tls + transport features), prost, prost-types, prost-reflect (serde feature, for proto3
  JSON), protox, tonic-reflection, tonic-types (google.rpc.Status details);
- path entries: websocket_client, grpc_client.
No build.rs and no protoc anywhere.

Create crates/websocket_client and crates/grpc_client: domain crates, no gpui, `[lints] workspace =
true`, lib roots src/websocket_client.rs and src/grpc_client.rs. Each depends on every crate declared
above for its protocol. Append to crates/http_engine/Cargo.toml every auth/proxy crate above that
http_engine will use.
Each crate gets src/tls_provider.rs: `pub fn ensure_crypto_provider()`. It installs the same rustls
CryptoProvider reqwest uses, once (a second call is a no-op). Tests:
- websocket_client: tls_provider_tests.rs, websocket_and_reqwest_share_one_crypto_provider. It
  builds a reqwest Client and a tokio-tungstenite rustls connector in one process, and each
  completes a TLS handshake to an in-process rcgen server.
- grpc_client: tls_provider_tests.rs, tonic_channel_and_reqwest_share_one_crypto_provider. Same
  shape, for a tonic Endpoint with TLS.

Append ARCHITECTURE.md rows for the two crates. Licences: every new package must be MIT, Apache-2.0,
BSD, ISC, Zlib, Unicode or MPL-2.0. Paste the licence-check output. Do not touch crates/pustman or
any existing test.
```

**Verify:**
- `scripts/pustman-v1-test.sh websocket_client tls_provider::` → `PASS 1`.
- `scripts/pustman-v1-test.sh grpc_client tls_provider::` → `PASS 1`.
- `cargo build -p http_engine` → exit 0.
- `cargo metadata --format-version 1 | jq -r '.packages[] | select(.source != null) | .license // "NONE"' | grep -ciE 'GPL|NONE'`
  → `0`. Our own crates have `source == null`, so they are not counted.
- `cargo tree -p grpc_client -p websocket_client -e normal | grep -c gpui` → `0`.
- `git ls-files '*build.rs' | wc -l` → the same count as on `pustman-v1/main` before the step.

### S50a — Request kinds for GraphQL, WebSocket, Socket.IO and gRPC
**Goal:** `Request` has `GraphQl`, `WebSocket`, `SocketIo` and `Grpc` variants that round-trip through
JSON, and every match site handles them.
**Depends on:** S49. **Deploy delta:** none.
**Edits:**
- `crates/api_model/src/protocol_request.rs`, `crates/api_model/src/protocol_request_tests.rs`;
- `crates/api_model/src/request.rs`;
- `crates/api_model/src/api_model.rs` (append `mod`/`pub use` lines only);
- every file a grep for `Request::Unsupported`/`Request::Http` match arms finds outside `api_model`.

```
Read crates/api_model/src/request.rs (the Request enum and its tagged serde), docs/PRD.md §5.2,
§5.4, §5.5, §5.6, §6.1.

src/protocol_request.rs defines:
- GraphQlRequest{id, name, url, headers, auth, query, variables: String (raw JSON text, so invalid
  JSON survives a save), operation_name: Option<String>, settings};
- WebSocketRequest{id, name, url, headers, params, subprotocols, auth, settings, saved_messages:
  Vec<SavedMessage{name, kind: Text|Json|Binary, payload}>};
- SocketIoRequest{id, name, url, path (default "/socket.io"), namespace (default "/"), headers,
  auth_payload: String, handshake_timeout_ms, listen: Vec<String>, all_events: bool (default true),
  saved_emits};
- GrpcRequest{id, name, target, service, method, message: String, metadata, auth, deadline_ms, tls:
  bool, descriptor_source: Reflection|ProtoFiles{files, import_paths}|DescriptorSet{path}}.
Reuse the existing header/param, Auth and RequestSettings types. Do not copy them.

request.rs: add the variants with serde tags "graphql", "websocket", "socketio" and "grpc".
Unsupported keeps every other kind (MQTT, …). Then run
`grep -rn "Request::Unsupported\|Request::Http" crates/ --include=*.rs` and handle the new variants
at every match site found. A site that cannot do anything for a kind yet treats it exactly as it
treats Unsupported today, and says so in a one-line comment. List every site you edited in a
Decision line.

Tests (protocol_request_tests.rs): graphql_request_round_trips_json,
websocket_request_round_trips_with_saved_messages, socketio_request_defaults_path_and_namespace,
grpc_request_round_trips_each_descriptor_source, mqtt_kind_still_preserved_as_unsupported.
Do not edit existing tests.
```

**Verify:**
- `scripts/pustman-v1-test.sh api_model protocol_request::` → `PASS 5`.
- `cargo check --workspace --all-targets` → exit 0.

### S50b — Workspace files per request kind
**Goal:** the workspace store saves and loads each new kind under its own suffix: `.graphql.json`,
`.websocket.json`, `.socketio.json` and `.grpc.json`. Files with these suffixes show in the tree.
**Depends on:** S50a. **Deploy delta:** none.
**Edits:** `crates/workspace_store/src/request_kinds.rs`, `crates/workspace_store/src/request_kinds_tests.rs`,
`crates/workspace_store/src/workspace_store.rs`.

```
Read docs/PRD.md §6.1, crates/workspace_store/src/workspace_store.rs (REQUEST_SUFFIX, Shape::Leaf,
the suffix matching, save_item; grep, since M2 changed this file). src/request_kinds.rs:
`suffix_for(&Request) -> &'static str` and `kind_for_file_name(&str) -> Option<RequestKind>`, the
one table of suffixes. workspace_store.rs: route save and load through it, so an item's file suffix
follows its kind. Saving an item whose kind changed renames the file atomically, like the existing
rename path. Deterministic JSON as today.

Tests (request_kinds_tests.rs): each_kind_saves_under_its_suffix,
snapshot_loads_every_kind_from_disk, unknown_suffix_is_not_a_request,
changing_kind_renames_the_file.
```

**Verify:** `scripts/pustman-v1-test.sh workspace_store request_kinds::` → `PASS 4`.

### S50c — Postman import and export of GraphQL requests
**Goal:** a Postman v2.1 request with `body.mode = "graphql"` imports as `Request::GraphQl` and
exports back unchanged. WebSocket, Socket.IO and gRPC items, which Postman stores outside v2.1, stay
preserved as before.
**Depends on:** S50a. **Deploy delta:** none. **High-stakes (import).**
**Edits:** `crates/api_model/src/postman_graphql.rs`, `crates/api_model/src/postman_graphql_tests.rs`,
`crates/api_model/src/postman.rs`, `crates/api_model/src/api_model.rs` (append `mod` only).

```
Read docs/PRD.md §6.4 (REQ-IO-01/06), crates/api_model/src/postman.rs (grep the import and export
item mapping). src/postman_graphql.rs: from_postman(item) -> Option<GraphQlRequest> when body.mode
is graphql, and to_postman(&GraphQlRequest) -> serde_json::Value.
- body.graphql.variables is a string in Postman. Keep it a string, invalid JSON included.
- Fields we do not model go into the item's preserved extras, as for HTTP.
postman.rs: call both functions; nothing else changes. Imported data is untrusted: no script
execution, size limits as today.

Tests (postman_graphql_tests.rs): graphql_body_imports_as_graphql_request,
graphql_request_exports_to_postman_graphql_body, invalid_variables_text_survives_round_trip.
```

**Verify:** `scripts/pustman-v1-test.sh api_model postman_graphql::` → `PASS 3`.

### S50d — "New" menu and actions for every request kind
**Goal:** the "+" new-tab button opens a "New" menu listing HTTP, GraphQL, WebSocket, Socket.IO and
gRPC (REQ-UI-03), and each entry is also a palette action. An entry whose kind has no editor yet is
disabled.
**Depends on:** S49. **Deploy delta:** none.
**Edits:**
- `crates/pustman_actions/src/pustman_actions.rs`;
- `crates/workspace/src/new_menu.rs`, `crates/workspace/src/new_menu_tests.rs`;
- `crates/workspace/src/pane_tabs.rs`;
- `crates/workspace/src/workspace.rs` (append `mod`/init lines only).

```
Load rust-gpui-ui-ux and gpui-actions-keymaps. Read crates/pustman_actions/src/pustman_actions.rs
(NewRequest), crates/workspace/src/pane_tabs.rs and workspace.rs (new_tab hands over to NewRequest),
crates/ui ContextMenu.

pustman_actions: add NewGraphQlRequest, NewWebSocketRequest, NewSocketIoRequest and NewGrpcRequest
next to NewRequest. Add no keybindings (the keymaps do not change), and leave out MQTT (STUB-M6).
src/new_menu.rs: a ContextMenu anchored on the "+" button, listing the five kinds. Each entry:
- dispatches its action;
- is disabled when `!cx.is_action_available(action)`, with a tooltip saying "Not available yet".
pane_tabs.rs: "+" opens the menu. Its existing Ctrl/Cmd-T path still opens HTTP directly.

Tests (new_menu_tests.rs): new_menu_lists_every_request_kind,
kind_without_editor_is_disabled, choosing_http_dispatches_new_request.
```

**Verify:** `scripts/pustman-v1-test.sh workspace new_menu::` → `PASS 3`.

### S50e — Open any request kind from the tree and quick open
**Goal:** opening a saved request of any kind, from the tree or from quick open, opens the editor for
that kind. A kind with no editor yet shows a toast that names the kind, instead of opening nothing.
**Depends on:** S50a, S50b. **Deploy delta:** none.
**Edits:**
- `crates/request_editor/src/open_item.rs`, `crates/request_editor/src/open_item_tests.rs`;
- `crates/request_editor/src/request_editor.rs` (append `mod`/init lines only);
- `crates/collections_panel/src/navigation.rs`;
- `crates/command_palette/src/quick_open.rs`.

```
Load rust-gpui-ui-ux. Read the current open path:
grep -rn "HttpRequest" crates/collections_panel/src/navigation.rs crates/command_palette/src/quick_open.rs
(S39b and S46). Also read crates/request_editor/src/new_request.rs.

src/open_item.rs:
- a global OpenerRegistry mapping RequestKind → fn(Request, &mut Window, &mut App) -> Result<()>;
- `register_opener(kind, f, cx)`;
- `open_item(request, window, cx)`. HTTP registers the existing opener. An unregistered kind returns
  an error that the caller shows as a toast naming the kind.
Re-opening an item already open focuses its tab, as S39b does for HTTP.
navigation.rs and quick_open.rs call open_item and nothing else changes in them. Do not touch any
other M2 file.

Tests (open_item_tests.rs): http_item_opens_through_registry,
unregistered_kind_reports_an_error_naming_it, registered_kind_opens_its_editor,
reopening_focuses_existing_tab.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor open_item::` → `PASS 4`.
- `cargo test -p collections_panel -p command_palette` → exit 0, so their existing tests still pass.

### S51a — Session events: the shared streaming model
**Goal:** the PRD §9 `SessionEvent` shape exists as plain data that serialises to one JSON line per
event, with redaction applied to payloads marked secret.
**Depends on:** S49. **Deploy delta:** none.
**Edits:** `crates/api_model/src/session.rs`, `crates/api_model/src/session_tests.rs`,
`crates/api_model/src/api_model.rs` (append `mod`/`pub use` only).

```
Read docs/PRD.md §9 (SessionEvent), §8.4 (REQ-LOG-01/02), crates/api_model/src/redact.rs.

src/session.rs:
- SessionEvent{Connecting{target}, Connected{details: ConnectionDetails}, Sent(Message),
  Received(Message), Error(SessionError), Closed{reason: CloseReason}};
- Message{at: timestamp ms, kind: Text|Json|Binary|Event, payload: Payload, size,
  fields: ProtocolFields};
- ProtocolFields: none / SocketIo{event, ack_id} / Grpc{message_index} / Sse{event, id, retry} /
  WebSocket{opcode}. Leave room for MQTT, but add no MQTT variant;
- Payload: Inline(Vec<u8>) or Spilled{path, len, prefix}. Large payloads are spilled by the store
  (S51b), not here;
- ConnectionDetails{request_headers, response_status, response_headers, protocol}, redacted with
  the existing redact();
- `to_jsonl_line(&SessionEvent) -> String`. Binary payloads are base64 (the base64 crate), and
  spilled payloads are read on export by S51c.
No GPUI.

Tests (session_tests.rs): each_event_serialises_to_one_json_line, binary_payload_exports_as_base64,
connection_details_redact_authorization.
```

**Verify:** `scripts/pustman-v1-test.sh api_model session::` → `PASS 3`.

### S51b — Message log store
**Goal:** a store holds 100,000 session events with bounded memory. Payloads over 1 MB live in temp
files and keep an in-memory preview, and search and filter come from incrementally built indices.
**Depends on:** S51a. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/session_store.rs`, `crates/response_viewer/src/session_store_tests.rs`,
`crates/response_viewer/src/response_viewer.rs` (append `mod` only), `crates/response_viewer/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-LOG-03/04, REQ-NFR-03, crates/http_engine/src/spill.rs (reuse its temp-file
spill and deletion; do not write a second one).

src/session_store.rs: SessionStore{events, …}.
- push(event): payloads over 1 MB are spilled through http_engine's spill, keeping a 4 KB prefix.
- filter(direction, kind, text) -> Vec<usize> of matching row indices, computed when data changes,
  never in render.
- clear(): deletes the spill files.
- export_jsonl(writer): streamed, so spilled payloads are copied, not loaded.
- Dropping the store deletes its temp files.

Tests (session_store_tests.rs): hundred_thousand_events_keep_inline_bytes_bounded (asserts the
inline byte total stays under a stated cap), payload_over_one_megabyte_is_spilled_with_prefix,
filter_by_direction_and_text, clear_deletes_spill_files, export_jsonl_streams_spilled_payloads.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer session_store::` → `PASS 5`.

### S51c — Message log view
**Goal:** the Message log renders a session as a virtualized list: direction, time, type, size and
preview. Expanding a row shows the full payload (pretty JSON, text or hex) and its protocol fields.
**Depends on:** S51b. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/session_log.rs`, `crates/response_viewer/src/session_log_tests.rs`,
`crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-LOG-01/02, REQ-WS-04, crates/ui VirtualList,
crates/response_viewer/src/pretty.rs and hex_view.rs (reuse; do not re-implement pretty-printing
or hex), and the tree's ten_thousand_items_render_only_visible_rows test (same assertion shape).

src/session_log.rs: SessionLog view over an Entity<SessionStore>.
- Rows: ↑ / ↓ / system glyph with an accessible label, absolute or relative time (a toggle),
  type, size, and a one-line preview. A truncated preview has the full value in a tooltip.
- Element ids come from the event sequence number, not the row index.
- Enter or a click expands a row. Expanded JSON is pretty-printed when the row is expanded, not in
  render, and spilled payloads show their prefix plus "Open full payload".
- Auto-scroll follows new rows.

Tests (session_log_tests.rs): hundred_thousand_events_render_only_visible_rows,
expanding_json_row_pretty_prints, binary_row_expands_to_hex,
socketio_row_shows_event_name_and_ack_id.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer session_log::` → `PASS 4`.

### S51d — Message log toolbar
**Goal:** the log can be searched, filtered by direction and type, paused, cleared, and its rows
copied; the session exports as JSON lines.
**Depends on:** S51c. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/log_toolbar.rs`, `crates/response_viewer/src/log_toolbar_tests.rs`,
`crates/response_viewer/src/session_log.rs`, `crates/response_viewer/src/response_viewer.rs` (append `mod` only).

```
Load rust-gpui-ui-ux and gpui-actions-keymaps. Read docs/PRD.md REQ-LOG-03,
crates/response_viewer/src/find.rs (reuse its search field pattern).

src/log_toolbar.rs:
- a search field and direction/type filter chips;
- Pause, which stops auto-scroll while events keep arriving, and Resume;
- Clear, and Copy (the payload of the selected row);
- Export, which writes the session to a JSONL file through a file dialog on a background task.
  A failure shows a toast.
Every control is keyboard-reachable and is an action with a palette name. session_log.rs only
mounts the toolbar.

Tests (log_toolbar_tests.rs): search_narrows_rows, pause_keeps_scroll_position_while_events_arrive,
clear_empties_log, export_writes_one_line_per_event.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer log_toolbar::` → `PASS 4`.

### S51e — One Console entry per session
**Goal:** every protocol session records one Console entry: the target, the handshake details,
message counts and the close reason, with secrets redacted.
**Depends on:** S51a. **Deploy delta:** none.
**Edits:** `crates/console/src/session_entry.rs`, `crates/console/src/session_entry_tests.rs`,
`crates/console/src/console.rs` (append `mod` only), `crates/console/Cargo.toml` (append deps).

```
Read crates/console/src/store.rs (ConsoleEntry has protocol: String), docs/PRD.md §8.1, REQ-WS-02,
REQ-NFR-04.

src/session_entry.rs: `SessionRecorder::new(protocol, target)` returns a ConsoleEntry. Feeding it
SessionEvents updates that entry:
- Connected records the redacted handshake;
- counts sent and received;
- Error and Closed record their reason.
Payloads are never copied into the Console, only counts and sizes.

Tests (session_entry_tests.rs): session_records_one_entry_with_counts,
failed_upgrade_records_status_and_body_size, handshake_authorization_is_redacted.
```

**Verify:** `scripts/pustman-v1-test.sh console session_entry::` → `PASS 3`.

---

# Phase 7 — M3: GraphQL, SSE and full auth

### S52a — Auth kinds: Digest, OAuth 2.0, AWS SigV4, JWT Bearer
**Goal:** `Auth` models Digest, OAuth 2.0 (four grants, implicit flagged legacy), AWS Signature v4 and
JWT Bearer. They round-trip through our JSON and Postman v2.1, and secrets stay keychain references.
**Depends on:** S50c. **Deploy delta:** none. **High-stakes (secrets, import).**
**Edits:**
- `crates/api_model/src/auth_extended.rs`, `crates/api_model/src/auth_extended_tests.rs`;
- `crates/api_model/src/auth.rs`;
- `crates/api_model/src/postman.rs`;
- `crates/api_model/src/api_model.rs` (append `mod` only).

```
Read docs/PRD.md REQ-HTTP-06/07, crates/api_model/src/auth.rs (Auth, effective_auth, Unsupported),
crates/api_model/src/postman.rs (auth mapping; grep).

src/auth_extended.rs:
- DigestAuth{username, password, realm?, nonce?, algorithm: Md5|Sha256 (+ -sess), qop?};
- OAuth2Auth{grant: AuthorizationCode{auth_url, use_pkce: bool (default true)}|ClientCredentials|
  Password{username, password}|Implicit{auth_url} (legacy), token_url, client_id, client_secret,
  scope, audience?, client_authentication: Header|Body, token_ref: Option<SecretRef>};
- AwsSigV4Auth{access_key, secret_key, session_token?, region, service};
- JwtBearerAuth{algorithm (HS256/384/512, RS256/384/512, ES256/384), secret_or_key,
  payload: String (JSON), header_prefix (default "Bearer"), add_to: Header|Query}.
Secret fields hold values or {{var}} references like Basic does today. Tokens live only in the
keychain (S56c). auth.rs gains the four variants and their kind() names; Unsupported keeps the rest
(Hawk, NTLM, OAuth1, …). postman.rs maps Postman's digest, oauth2, awsv4 and jwt auth types both ways.

Tests (auth_extended_tests.rs): each_new_kind_round_trips_json,
postman_oauth2_imports_with_grant_and_pkce_flag, implicit_grant_marked_legacy,
postman_awsv4_and_jwt_round_trip, ntlm_stays_unsupported.
```

**Verify:** `scripts/pustman-v1-test.sh api_model auth_extended::` → `PASS 5`.

### S52b — Network model: proxy, client certificates, CA bundle
**Goal:** settings can express:
- a proxy: system, manual (HTTP, HTTPS or SOCKS5, auth in the keychain) or none, with `NO_PROXY`;
- client certificates per host: PEM, or PFX with its passphrase in the keychain;
- a custom CA bundle;
- a per-request proxy bypass.
**Depends on:** S49. **Deploy delta:** none.
**Edits:**
- `crates/api_model/src/network.rs`, `crates/api_model/src/network_tests.rs`;
- `crates/api_model/src/settings.rs`;
- `crates/api_model/src/api_model.rs` (append `mod` only);
- `assets/settings/default.json`.

```
Read docs/PRD.md REQ-HTTP-10/11, REQ-PROXY-02, crates/api_model/src/settings.rs (RequestSettings),
assets/settings/default.json, the pustman-settings-persistence skill.

src/network.rs:
- ProxyConfig{mode: System|Manual{scheme: Http|Https|Socks5, host, port, auth_secret:
  Option<SecretRef>}|None, no_proxy: Vec<String>};
- ClientCertificate{host_pattern, kind: Pem{cert_path, key_path}|Pfx{path,
  passphrase_secret: Option<SecretRef>}};
- NetworkSettings{proxy, client_certificates, ca_bundle_path: Option<PathBuf>}, with serde defaults
  (proxy System, no certs, no CA);
- `matches_no_proxy(host)` and `certificate_for(host)`.
No secret value is ever stored here, only references. RequestSettings gains
`proxy_bypass: Option<bool>`. default.json gains a "network" section with the defaults.

Tests (network_tests.rs): defaults_are_system_proxy_and_no_certs, no_proxy_matches_suffix_and_exact,
certificate_for_host_picks_most_specific, network_settings_never_serialise_a_secret_value.
```

**Verify:** `scripts/pustman-v1-test.sh api_model network::` → `PASS 4`.

### S53 — GraphQL over the HTTP engine
**Goal:** a `GraphQlRequest` sends as `POST application/json {query, variables, operationName}`
through the normal pipeline. It shows in History and the Console as protocol `graphql`, and a document
with several operations lists their names.
**Depends on:** S50a. **Deploy delta:** none.
**Edits:**
- `crates/http_engine/src/graphql.rs`, `crates/http_engine/src/graphql_tests.rs`;
- `crates/http_engine/src/client.rs`;
- `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read docs/PRD.md REQ-GQL-01, crates/http_engine/src/client.rs (send_request; grep), history.rs
(PROTOCOL const; "plan D7 gives later protocols their own row shapes"), prepare.rs.

src/graphql.rs:
- `to_http_request(&GraphQlRequest) -> Result<HttpRequest, GraphQlError>`. Invalid variables JSON
  is an error naming the line and column, and {{vars}} inside the query and variables resolve via
  the normal prepare;
- `operation_names(query) -> Vec<String>`, a small scanner for `query|mutation|subscription Name`
  that skips comments and strings. Do not pull in a GraphQL parser crate for this.
client.rs: `send_graphql(...)` reuses send_request and records history and Console with protocol
"graphql".

Tests (graphql_tests.rs): posts_query_variables_and_operation_name,
invalid_variables_json_reports_line_and_column, operation_names_listed_in_document_order,
history_records_protocol_graphql.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine graphql::` → `PASS 4`.

### S54a — Server-sent events parser
**Goal:** an incremental parser turns `text/event-stream` bytes into events (`event`, `id`, `data`,
`retry`) per the WHATWG rules, across arbitrary chunk boundaries.
**Depends on:** S51a. **Deploy delta:** none.
**Edits:** `crates/http_engine/src/event_source.rs`, `crates/http_engine/src/event_source_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read docs/PRD.md REQ-SSE-01 and the WHATWG HTML "event stream interpretation" rules (Context7 or the
spec).

src/event_source.rs: EventSourceParser::feed(&[u8]) -> Vec<SseEvent>.
- Handles CRLF/LF/CR line ends, multi-line data, comment lines (":"), and a BOM at the start.
- A field without a colon is a field with an empty value; `retry` takes digits only.
- A trailing partial line is buffered until its line ends.
- `finish()` handles the stream end: an unterminated event is dropped, per the spec.
Mapping SseEvent → session::Message with ProtocolFields::Sse goes here too.

Tests (event_source_tests.rs): multi_line_data_joined_with_newlines,
event_split_across_every_byte_boundary, comments_and_bom_ignored, retry_accepts_digits_only.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine event_source::` → `PASS 4`.

### S54b — Stream an event-stream response
**Goal:** a response with `Content-Type: text/event-stream` streams its events as `SessionEvent`s until
the server closes it or the user stops it. Stopping closes the connection.
**Depends on:** S54a, S53. **Deploy delta:** none.
**Edits:**
- `crates/http_engine/src/event_stream.rs`, `crates/http_engine/src/event_stream_tests.rs`;
- `crates/http_engine/src/reqwest_transport.rs`;
- `crates/http_engine/src/client.rs`;
- `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read crates/http_engine/src/reqwest_transport.rs (body streaming, spill; grep), client.rs (cancel),
transport.rs, test_server.rs.

reqwest_transport.rs: when the response content type is text/event-stream, hand the body stream to
event_stream instead of buffering or spilling it. The Response carries
`ResponseBody::EventStream` (new variant) with a receiver of SessionEvent.

src/event_stream.rs:
- the receiver side;
- `stop()` drops the stream, which closes the connection;
- a Closed{reason} event when the server ends or the user stops.
Keep the variant's derives consistent with ResponseBody; match it everywhere grep finds a
ResponseBody match. client.rs: cancel() stops an open stream.

Tests (event_stream_tests.rs, using the in-process test_server sending SSE):
events_arrive_before_the_stream_ends, server_close_emits_closed, stop_closes_the_connection.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine event_stream::` → `PASS 3`.

### S54c — Event-stream mode in the response viewer
**Goal:** an SSE response switches the response viewer into a live Message log with a Stop control,
and 10,000 events stay smooth.
**Depends on:** S54b, S51d. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/event_stream_view.rs`, `crates/response_viewer/src/event_stream_view_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`.

```
Load rust-gpui-ui-ux. Read crates/response_viewer/src/response_viewer.rs (body modes; grep),
session_log.rs, log_toolbar.rs.

src/event_stream_view.rs: for ResponseBody::EventStream, feed a SessionStore from the receiver on a
stored Task, and render SessionLog with the toolbar plus a Stop button (an action). The status line
shows "Streaming…" until Closed, then the close reason. response_viewer.rs picks this view for the
variant.

Tests (event_stream_view_tests.rs): sse_response_shows_live_log_rows,
stop_button_closes_stream_and_shows_reason, ten_thousand_sse_events_render_only_visible_rows.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer event_stream_view::` → `PASS 3`.

### S55a — Digest auth
**Goal:** a Digest request answers the server's 401 challenge once, with MD5 or SHA-256 (and their
`-sess` forms), `qop=auth` and a nonce count. A second 401 is returned to the user.
**Depends on:** S52a, S54b. **Deploy delta:** none. **High-stakes (auth).**
**Edits:**
- `crates/http_engine/src/digest.rs`, `crates/http_engine/src/digest_tests.rs`;
- `crates/http_engine/src/client.rs`;
- `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read RFC 7616, crates/http_engine/src/request_auth.rs, client.rs (send path; grep), test_server.rs.
Use the digest crate S49 declared. Do not hand-roll MD5/SHA-256.

src/digest.rs: parse WWW-Authenticate, build the Authorization header, and compute the response per
RFC 7616 (its §3.9.1 example is a test vector). client.rs: a Digest-auth request that gets a 401
with a Digest challenge retries exactly once with the computed header. Never log the header.

Tests (digest_tests.rs): rfc7616_md5_vector_matches, rfc7616_sha256_vector_matches,
challenge_answered_once_then_401_returned, digest_header_never_logged.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine digest::` → `PASS 4`.

### S55b — AWS Signature v4
**Goal:** a request with AWS SigV4 auth is signed per AWS's test suite: canonical request,
`X-Amz-Date`, optional session token, and an unsigned-payload option.
**Depends on:** S52a. **Deploy delta:** none. **High-stakes (auth).**
**Edits:** `crates/http_engine/src/sigv4.rs`, `crates/http_engine/src/sigv4_tests.rs`,
`crates/http_engine/tests/fixtures/sigv4/*` (new), `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read the aws-sigv4 crate docs via Context7 (declared in S49), docs/PRD.md REQ-HTTP-06,
crates/http_engine/src/request_auth.rs.

src/sigv4.rs: `sign(&AwsSigV4Auth, &mut ResolvedRequest, now)` through aws-sigv4. Do not
re-implement canonicalisation. The clock is injected so tests are deterministic.

Tests (sigv4_tests.rs), using the AWS SigV4 test-suite cases checked in under crates/http_engine/tests/fixtures/sigv4/ with
their provenance noted: get_vanilla_matches_aws_suite, post_with_body_matches_aws_suite,
session_token_adds_security_token_header.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine sigv4::` → `PASS 3`.

### S55c — JWT Bearer
**Goal:** a JWT Bearer request signs its payload with the chosen algorithm and key, and adds the token
as a header with the chosen prefix, or as a query parameter.
**Depends on:** S52a. **Deploy delta:** none. **High-stakes (auth).**
**Edits:** `crates/http_engine/src/jwt.rs`, `crates/http_engine/src/jwt_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read the jsonwebtoken crate via Context7, crates/http_engine/src/request_auth.rs.

src/jwt.rs: `sign(&JwtBearerAuth) -> Result<String, JwtError>`.
- An invalid payload JSON or key is an error naming which.
- iat/exp are not added unless the payload says so.
Never log the token or the key.

Tests (jwt_tests.rs): hs256_token_verifies_with_same_secret, rs256_token_verifies_with_public_key,
invalid_payload_json_names_the_problem.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine jwt::` → `PASS 3`.

### S55d — Apply the new auth kinds when sending
**Goal:** sending a request with Digest, SigV4 or JWT auth applies it in the pipeline, after variable
resolution and before the request leaves. History and the Console show the auth kind without its
secrets.
**Depends on:** S55a, S55b, S55c. **Deploy delta:** none. **High-stakes (auth).**
**Edits:** `crates/http_engine/src/auth_pipeline.rs`, `crates/http_engine/src/auth_pipeline_tests.rs`,
`crates/http_engine/src/request_auth.rs`, `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read crates/http_engine/src/request_auth.rs (apply_auth), digest.rs, sigv4.rs, jwt.rs, redact usage.
request_auth.rs: apply_auth dispatches the three kinds; Digest marks the request for the client's
challenge path. src/auth_pipeline.rs holds the dispatch helpers.

Tests (auth_pipeline_tests.rs; FakeTransport): sigv4_request_leaves_signed,
jwt_header_prefix_applied, auth_secrets_absent_from_history_and_console.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine auth_pipeline::` → `PASS 3`.

### S56a — OAuth 2.0: client credentials and password grants
**Goal:** "Get new access token" for the client-credentials and password grants POSTs to the token
URL through our engine, with client auth as a Basic header or in the body. It returns the token,
expiry and refresh token.
**Depends on:** S55d. **Deploy delta:** none. **High-stakes (auth).**
**Edits:** `crates/http_engine/src/oauth2_grants.rs`, `crates/http_engine/src/oauth2_grants_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read RFC 6749 §4.3/§4.4/§5, docs/PRD.md REQ-HTTP-07, crates/http_engine/src/client.rs (send path).
OAuth runs on our own engine (Phase 6 decision); do not add an OAuth client crate.

src/oauth2_grants.rs: `request_token(&OAuth2Auth, transport) -> Result<TokenSet{access_token,
token_type, expires_at, refresh_token?, scope?}, OAuthError>`.
- An error response maps RFC 6749 §5.2 `error`/`error_description` into OAuthError, which the UI
  shows.
- `refresh(&TokenSet)` runs the refresh_token grant.
No token or secret in logs.

Tests (oauth2_grants_tests.rs; in-process test server as IdP): client_credentials_returns_token_set,
password_grant_sends_credentials_in_body, client_auth_header_mode_uses_basic,
error_response_maps_rfc6749_error.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine oauth2_grants::` → `PASS 4`.

### S56b — OAuth 2.0: authorization code with PKCE
**Goal:** the authorization-code grant:
- opens the system browser to the authorize URL with PKCE S256 and a random `state`;
- receives the code on a one-shot loopback `http://127.0.0.1:<port>/callback`;
- checks `state`;
- exchanges the code.
The implicit grant works too and is flagged legacy.
**Depends on:** S56a. **Deploy delta:** none. **High-stakes (auth).**
**Edits:** `crates/http_engine/src/oauth2_pkce.rs`, `crates/http_engine/src/oauth2_pkce_tests.rs`,
`crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read RFC 7636, RFC 8252 §7.3 (loopback redirect), docs/PRD.md REQ-HTTP-07 (system browser, no
embedded browser).

src/oauth2_pkce.rs:
- code_verifier from rand, 43–128 chars; challenge = base64url(sha256(verifier)), using the sha2 and
  base64 crates;
- a loopback listener bound to 127.0.0.1:0 that accepts one /callback request. It answers with a
  small "You can close this tab" page and shuts down. It rejects a mismatched state, and times out
  after 5 minutes (injectable);
- `authorize(&OAuth2Auth, open_url: impl Fn(Url)) -> Result<TokenSet>`. The caller passes
  cx.open_url, and tests pass a fake that drives the callback.
Implicit grant: token from the fragment, via the same listener serving a page that posts the
fragment back.

Tests (oauth2_pkce_tests.rs): challenge_is_s256_of_verifier, callback_with_wrong_state_rejected,
code_exchanged_with_verifier, listener_binds_loopback_only.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine oauth2_pkce::` → `PASS 4`.

### S56c — OAuth 2.0 tokens: keychain and refresh
**Goal:** obtained tokens are stored only in the keychain. A request using OAuth 2.0 sends the stored
token, and an expired token is refreshed before the send when a refresh token exists. When none
exists, the send stops with an actionable error.
**Depends on:** S56b. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:**
- `crates/http_engine/src/oauth2_tokens.rs`, `crates/http_engine/src/oauth2_tokens_tests.rs`;
- `crates/http_engine/src/client.rs`;
- `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read crates/secrets (SecretStore, MemorySecretStore), crates/http_engine/src/client.rs.

src/oauth2_tokens.rs: TokenSet ↔ SecretStore under a key derived from the request id plus the grant.
Only expires_at and the token type are kept outside the keychain, so the UI can show token status.
client.rs:
- before sending an OAuth 2.0 request, load the token and refresh it if it expires within 30 s;
- on no token or a failed refresh, return SendError::OAuthTokenMissing{reason};
- apply as `Authorization: <token_type> <token>`.

Tests (oauth2_tokens_tests.rs): token_saved_only_in_secret_store, expired_token_refreshed_before_send,
missing_token_fails_with_actionable_error.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine oauth2_tokens::` → `PASS 3`.

### S56d — M3 OAuth AC: PKCE against a local test IdP, and OAuth tokens in the redaction test
**Goal:** the M3 exit criterion "OAuth 2.0 PKCE against a local test IdP" passes end to end in a test,
and OAuth tokens join the redaction fixture.
**Depends on:** S56c. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/integration_tests/tests/oauth_pkce.rs`, `crates/integration_tests/tests/oauth_redaction.rs`,
`crates/integration_tests/Cargo.toml` (append deps).

```
Read docs/PRD.md §11 M3, §10 "Redaction test", crates/integration_tests/tests/redaction.rs (read
only; do not edit it).

tests/oauth_pkce.rs: an in-process test IdP that issues codes only for a matching S256 challenge
and redirects to the loopback callback. It drives authorize() with a fake browser that follows the
redirect, then sends a request to a protected endpoint that requires the issued token. Test:
oauth_pkce_against_local_idp.

tests/oauth_redaction.rs: obtain a token and refresh token, send, record history and export the
Console. Assert that neither value appears in any workspace file, the SQLite database or the
export. Test: oauth_tokens_never_persist_outside_keychain.
```

**Verify:**
- `scripts/pustman-v1-test.sh integration_tests oauth_pkce_against_local_idp` → `PASS 1`.
- `scripts/pustman-v1-test.sh integration_tests oauth_tokens_never_persist_outside_keychain` → `PASS 1`.

### S57a — Proxy
**Goal:** requests go through the configured proxy (system, manual HTTP/HTTPS/SOCKS5 with keychain
auth, or none). `NO_PROXY` and the per-request bypass are honoured, and the Console shows which
proxy was used.
**Depends on:** S52b, S54b. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:**
- `crates/http_engine/src/proxy.rs`, `crates/http_engine/src/proxy_tests.rs`;
- `crates/http_engine/src/reqwest_transport.rs`;
- `crates/http_engine/src/request.rs`;
- `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read docs/PRD.md REQ-HTTP-11, crates/http_engine/src/reqwest_transport.rs (client building and
caching; grep), request.rs (SendSettings), crates/api_model/src/network.rs.

SendSettings gains `network: NetworkSettings` and `proxy_bypass`. src/proxy.rs:
- builds reqwest::Proxy values. System reads the environment's HTTP(S)_PROXY/NO_PROXY through
  reqwest's system proxy;
- Manual resolves its password from the SecretStore;
- bypass and NO_PROXY skip the proxy.
reqwest_transport.rs keys its cached clients by the network config.

Tests (proxy_tests.rs; in-process HTTP CONNECT proxy and SOCKS5 test servers):
manual_http_proxy_used, socks5_proxy_used, no_proxy_host_goes_direct, per_request_bypass_goes_direct,
proxy_password_comes_from_secret_store.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine proxy::` → `PASS 5`.

### S57b — Client certificates and a custom CA bundle
**Goal:** a host with a configured client certificate (PEM, or PFX with a keychain passphrase) passes
mutual TLS. A custom CA bundle is trusted in addition to the system roots, and TLS verification stays
on unless disabled per request.
**Depends on:** S57a. **Deploy delta:** none. **High-stakes (TLS, secrets).**
**Edits:** `crates/http_engine/src/client_identity.rs`, `crates/http_engine/src/client_identity_tests.rs`,
`crates/http_engine/src/reqwest_transport.rs`, `crates/http_engine/src/http_engine.rs` (append `mod` only).

```
Read docs/PRD.md REQ-HTTP-10, crates/http_engine/src/reqwest_transport.rs (S28c's TLS handling; grep),
the p12-keystore crate via Context7.

src/client_identity.rs:
- PEM files are read on a background task;
- PFX is decoded with p12-keystore and its passphrase from the SecretStore, then converted to a
  reqwest Identity;
- the CA bundle is added as extra roots.
An error names the file and the cause ("wrong passphrase", "not a PFX file"). reqwest_transport
picks the identity per host via certificate_for(host).

Tests (client_identity_tests.rs; rcgen CA + an in-process mTLS server):
pem_identity_passes_mutual_tls, pfx_with_keychain_passphrase_passes, wrong_passphrase_names_the_file,
custom_ca_trusts_private_server.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine client_identity::` → `PASS 4`.

### S57c — Proxy and certificate secrets from the palette; per-request bypass toggle
**Goal:** the user can:
- set or clear the proxy password and each PFX passphrase from the command palette, stored in the
  keychain;
- toggle "Bypass proxy" in a request's Settings tab.
**Depends on:** S57b, S50d. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:**
- `crates/request_editor/src/network_secrets.rs`, `crates/request_editor/src/network_secrets_tests.rs`;
- `crates/request_editor/src/settings_tab.rs`;
- `crates/request_editor/src/request_editor.rs` (append `mod`/init lines only);
- `crates/pustman_actions/src/pustman_actions.rs`.

```
Load rust-gpui-ui-ux. Read crates/request_editor/src/settings_tab.rs, crates/secrets, docs/PRD.md
REQ-HTTP-10/11.

pustman_actions: SetProxyPassword and SetCertificatePassphrase.
src/network_secrets.rs: each action opens a masked-input modal and writes to the SecretStore on a
background task. A failure shows a toast. The value is never logged or kept in a view field after
the write.
settings_tab.rs adds a "Bypass proxy" toggle bound to RequestSettings.proxy_bypass.

Tests (network_secrets_tests.rs): proxy_password_written_to_secret_store,
passphrase_modal_clears_value_after_save, bypass_toggle_saves_setting.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor network_secrets::` → `PASS 3`.

### S58a — Auth tab: Digest, AWS SigV4 and JWT forms
**Goal:** the Auth tab's type picker offers Digest, AWS Signature v4 and JWT Bearer, each with an
aligned form that supports `{{var}}` fields.
**Depends on:** S52a, S57c. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/auth_forms.rs`, `crates/request_editor/src/auth_forms_tests.rs`,
`crates/request_editor/src/auth_tab.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read crates/request_editor/src/auth_tab.rs (current aligned-form layout; grep),
variable_highlight.rs, docs/postman_redesign_pack/01_specs/screen_specs.md (auth section).

src/auth_forms.rs: one form builder per kind, using the label-column layout auth_tab already uses.
Secret fields are masked with a reveal toggle. auth_tab.rs adds the kinds to its picker and calls
the builders.

Tests (auth_forms_tests.rs): picker_offers_digest_sigv4_jwt, sigv4_form_edits_region_and_service,
jwt_algorithm_change_updates_auth, secret_fields_masked_by_default.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor auth_forms::` → `PASS 4`.

### S58b — Auth tab: OAuth 2.0
**Goal:** the OAuth 2.0 form:
- picks a grant (implicit labelled "legacy");
- edits URLs, client, scope and PKCE;
- has "Get new access token", which runs the flow;
- shows token status (valid until …, expired, none) without showing the token.
**Depends on:** S58a, S56c. **Deploy delta:** none. **High-stakes (secrets).**
**Edits:** `crates/request_editor/src/oauth2_form.rs`, `crates/request_editor/src/oauth2_form_tests.rs`,
`crates/request_editor/src/auth_tab.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read crates/request_editor/src/auth_tab.rs, auth_forms.rs,
crates/http_engine/src/oauth2_grants.rs, oauth2_pkce.rs, oauth2_tokens.rs.

src/oauth2_form.rs:
- "Get new access token" runs on a stored Task, and the browser grants call cx.open_url;
- progress shows while it waits for the callback, and Cancel stops it;
- errors (OAuthError description) show inline;
- token status reads expires_at only;
- "Clear token" deletes it from the keychain.
auth_tab.rs adds OAuth 2.0 to the picker.

Tests (oauth2_form_tests.rs; fake IdP transport): get_token_shows_valid_until,
implicit_grant_labelled_legacy, idp_error_shown_inline, clear_token_removes_it.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor oauth2_form::` → `PASS 4`.

### S59a — GraphQL editor
**Goal:** a GraphQL tab has the URL, headers and auth, a query editor, a variables JSON editor with
inline validation, and an operation picker when the document has several operations. Send runs
it, and `NewGraphQlRequest` opens one.
**Depends on:** S53, S50d, S50e, S58b. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/graphql_editor.rs`, `crates/request_editor/src/graphql_editor_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-GQL-01, crates/request_editor/src/request_editor.rs (tab
item shape), new_request.rs, headers.rs, auth_tab.rs, open_item.rs (register_opener),
crates/text_input (multi-line), crates/http_engine/src/graphql.rs.

src/graphql_editor.rs: a GraphQlEditor item that reuses the URL bar, headers table and auth tab. It
adds:
- the query and variables editors with {{var}} highlighting;
- a variables error that names line and column;
- an operation picker (hidden when there is one operation).
Send calls send_graphql and shows the result in the response viewer. init registers the
NewGraphQlRequest handler and the GraphQl opener. Save writes a .graphql.json item.

Tests (graphql_editor_tests.rs): new_graphql_request_opens_editor,
invalid_variables_shows_line_and_column, operation_picker_lists_names_when_several,
send_posts_graphql_body.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor graphql_editor::` → `PASS 4`.

### S59b — GraphQL errors panel
**Goal:** a GraphQL response with an `errors[]` array shows the errors in their own panel above
`data`, with message, path and locations, even when the HTTP status is 200.
**Depends on:** S54c. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/graphql_errors.rs`, `crates/response_viewer/src/graphql_errors_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-GQL-05, crates/response_viewer/src/response_viewer.rs,
error_panel.rs.

src/graphql_errors.rs: parse errors[] when the response is JSON (computed on data change, not in
render). Render a warning-severity panel listing each error's message, path joined with "." and
line:column. Partial data still shows below. response_viewer.rs mounts it for responses of
protocol graphql.

Tests (graphql_errors_tests.rs): errors_shown_on_http_200, partial_data_still_rendered,
no_errors_hides_panel.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer graphql_errors::` → `PASS 3`.

### S59w — Wire GraphQL into the app (M3 E2E)
**Goal:** in the app as `pustman::init` builds it, `NewGraphQlRequest` opens a GraphQL tab whose
Send reaches a local server and shows the `errors[]` panel.
**Depends on:** S59a, S59b, S56d. **Deploy delta:** none.
**Edits:** `crates/pustman/tests/wire_graphql.rs`, `crates/pustman/Cargo.toml` (hotspot; append dev-deps only).

```
Read crates/pustman/tests/wire_request_editor.rs (same shape). tests/wire_graphql.rs: init the app
with the in-memory store, dispatch NewGraphQlRequest, type a query against an in-process server that
answers 200 with errors[], send, and assert the errors panel. Test:
wire_graphql_new_request_sends_and_shows_errors.
Do not edit crates/pustman/src (request_editor and response_viewer are wired already).
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_graphql` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S60 — M3 SIGN-OFF (HUMAN)
**Goal:** the M3 exit criteria are proven on real machines.
**Depends on:** S54c, S57c, S58b, S59w. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux and macOS.
- GraphQL against a public API: variables, several operations, and an errors[] response on 200.
- An SSE endpoint streams 10k events smoothly, and Stop closes it.
- OAuth 2.0 auth code + PKCE against a real IdP (e.g. a local Keycloak) through the system browser.
  The token survives a restart via the keychain and refreshes when expired.
- Digest against httpbin's /digest-auth.
- An SigV4 request to an AWS endpoint (or a recorded signature).
- Manual proxy and SOCKS5.
- A client certificate against an mTLS test host.
Record M3 GO/NO-GO, then /promote → the next free release tag (Phase 6 rule).
```

**Verify:** the log records `M3 GO`. CI is green at the merge SHA.

---

# Phase 8 — M4: WebSocket and Socket.IO

### S61a — WebSocket connect
**Goal:** `websocket_client::connect(request, network)` opens `ws://` or `wss://` with the
request's headers, query params, subprotocols, auth, TLS, client certificate and proxy settings. It
returns a handle and a `SessionEvent` stream, and dropping the stream closes the connection.
**Depends on:** S50a, S51a, S52b. **Deploy delta:** none.
**Edits:** `crates/websocket_client/src/connect.rs`, `crates/websocket_client/src/connect_tests.rs`,
`crates/websocket_client/src/test_echo_server.rs`, `crates/websocket_client/src/websocket_client.rs` (append `mod` only),
`crates/websocket_client/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-WS-01, §9 (SessionEvent; connect returns a handle and a stream),
crates/api_model/src/session.rs and network.rs, tokio-tungstenite via Context7, AGENTS.md (no
secrets in logs).

src/connect.rs: build the upgrade request (Basic, Bearer or API key as header or query via the
existing auth helpers), apply the TLS/cert/proxy settings (reuse http_engine's identity and proxy
builders through a pub fn; do not copy them), and connect on gpui_tokio's runtime handle passed in.
- Emit Connecting, then Connected{details} with the 101 headers.
- `SessionHandle::send(Message)` and `close(code, reason)`.
src/test_echo_server.rs (cfg(any(test, feature = "test-support"))): an in-process echo server that
records the upgrade headers.

Tests (connect_tests.rs): connects_with_headers_and_subprotocol, bearer_auth_sent_on_upgrade,
dropping_stream_closes_connection.
```

**Verify:** `scripts/pustman-v1-test.sh websocket_client connect::` → `PASS 3`.

### S61b — WebSocket messages and close
**Goal:** the user can send Text, JSON (validated) and Binary messages (from Base64, hex or a file) and
receive them. Ping and pong surface as events, and closing with a code and reason (or a server
close) emits Closed with both.
**Depends on:** S61a. **Deploy delta:** none.
**Edits:** `crates/websocket_client/src/frames.rs`, `crates/websocket_client/src/frames_tests.rs`,
`crates/websocket_client/src/websocket_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-WS-03/05, crates/websocket_client/src/connect.rs.

src/frames.rs: Composer → Message.
- JSON is validated, and an error names line and column.
- Binary is parsed from base64 or hex with the base64/hex crates; a file is read on a background
  task.
- Received frames map to Received with ProtocolFields::WebSocket{opcode}.
- Ping and pong are events flagged so the log can hide them.
- Closed{code, reason} is emitted for a local or remote close.

Tests (frames_tests.rs): text_json_binary_echoed, invalid_json_rejected_before_send,
close_code_and_reason_reported, ping_pong_events_emitted.
```

**Verify:** `scripts/pustman-v1-test.sh websocket_client frames::` → `PASS 4`.

### S61c — WebSocket handshake failures
**Goal:** a refused upgrade reports the HTTP status and body. TLS and DNS errors are actionable
`SessionError`s.
**Depends on:** S61a. **Deploy delta:** none.
**Edits:** `crates/websocket_client/src/handshake.rs`, `crates/websocket_client/src/handshake_tests.rs`,
`crates/websocket_client/src/websocket_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-WS-02, crates/http_engine/src/error.rs (reuse its error taxonomy wording).
src/handshake.rs: map tungstenite's Http(response) error to SessionError::UpgradeRefused{status,
headers, body_prefix}, and map TLS and connect errors to the same wording the HTTP engine uses.

Tests (handshake_tests.rs): non_101_reports_status_and_body, tls_error_is_actionable.
```

**Verify:** `scripts/pustman-v1-test.sh websocket_client handshake::` → `PASS 2`.

### S62a — Socket.IO connect
**Goal:** a Socket.IO v4 session connects with path, namespace, headers, auth payload and handshake
timeout, and logs the lifecycle events `connect`, `connect_error` and `disconnect` with its reason.
**Depends on:** S61a. **Deploy delta:** none.
**Edits:** `crates/websocket_client/src/socketio.rs`, `crates/websocket_client/src/socketio_tests.rs`,
`crates/websocket_client/src/test_socketio_server.rs`, `crates/websocket_client/src/websocket_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-SIO-01/04, rust_socketio and socketioxide via Context7, and S49's Decision line
on supported protocol versions.

src/socketio.rs: `connect_socketio(&SocketIoRequest, network) -> (SocketIoHandle, stream of
SessionEvent)` via rust_socketio's async client. Lifecycle events are system rows.
src/test_socketio_server.rs (test-support): a socketioxide server on hyper that records the auth
payload, rejects a bad token, and echoes.

Tests (socketio_tests.rs): connects_to_namespace_with_auth_payload, rejected_auth_emits_connect_error,
server_disconnect_reports_reason.
```

**Verify:** `scripts/pustman-v1-test.sh websocket_client socketio::` → `PASS 3`.

### S62b — Socket.IO emit, acknowledgements and listeners
**Goal:** the user can emit an event with one or more Text, JSON or Binary arguments and an optional
ack, with the ack reply linked to its emit. Received events log with their name, filtered by the
listen list unless all-events mode is on.
**Depends on:** S62a. **Deploy delta:** none.
**Edits:** `crates/websocket_client/src/socketio_emit.rs`, `crates/websocket_client/src/socketio_emit_tests.rs`,
`crates/websocket_client/src/websocket_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-SIO-02/03, crates/websocket_client/src/socketio.rs.
src/socketio_emit.rs: `emit(event, args, ack: bool)`. The Sent row carries ack_id, and the ack
reply is a Received row with the same ack_id. The listener filter applies the listen list, or all
events.

Tests (socketio_emit_tests.rs): emit_with_ack_links_reply, multiple_args_sent_in_order,
listen_list_filters_events, all_events_mode_receives_everything.
```

**Verify:** `scripts/pustman-v1-test.sh websocket_client socketio_emit::` → `PASS 4`.

### S63a — WebSocket editor
**Goal:** a WebSocket tab has URL, headers, params, subprotocols and auth, plus Connect/Disconnect
(with code and reason), a composer (Text, JSON or Binary), and the live Message log with ping/pong
behind a toggle. `NewWebSocketRequest` opens one.
**Depends on:** S61b, S61c, S51d, S50d, S50e. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/websocket_editor.rs`, `crates/request_editor/src/websocket_editor_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only), `crates/request_editor/Cargo.toml` (append deps).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-WS-01..05, crates/request_editor/src/graphql_editor.rs
if merged (same tab shape), otherwise request_editor.rs. Also read
crates/response_viewer/src/session_log.rs and log_toolbar.rs, crates/websocket_client.

src/websocket_editor.rs:
- The session runs on a stored Task, and closing the tab drops it, which disconnects.
- Connection state shows in the tab title and status (connecting, connected, closed with code).
- Errors (UpgradeRefused etc.) show inline and in the Console.
init registers the NewWebSocketRequest handler and the WebSocket opener. Save writes a
.websocket.json item.

Tests (websocket_editor_tests.rs; test echo server): connect_send_and_receive_in_log,
disconnect_with_code_and_reason, closing_tab_disconnects, ping_pong_hidden_until_toggled.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor websocket_editor::` → `PASS 4`.

### S63b — Saved messages
**Goal:** a composed WebSocket message can be saved by name inside the request, and resent with one
click or from the keyboard.
**Depends on:** S63a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/saved_messages.rs`, `crates/request_editor/src/saved_messages_tests.rs`,
`crates/request_editor/src/websocket_editor.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-WS-03, crates/request_editor/src/websocket_editor.rs.

src/saved_messages.rs: a list of saved messages with send, rename and delete, stored in
WebSocketRequest.saved_messages and written on save. The list is keyboard-navigable.
websocket_editor.rs mounts it.

Tests (saved_messages_tests.rs): save_composed_message_by_name, resend_saved_message_with_enter,
saved_messages_persist_in_request_file.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor saved_messages::` → `PASS 3`.

### S64 — Socket.IO editor
**Goal:** a Socket.IO tab has URL, path, namespace, headers, auth payload and timeout; an emit composer
(event, arguments, ack); a listen list with an all-events toggle; and the Message log showing event
names and acks. `NewSocketIoRequest` opens one.
**Depends on:** S62b, S63a. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/socketio_editor.rs`, `crates/request_editor/src/socketio_editor_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-SIO-01..04, crates/request_editor/src/websocket_editor.rs
(reuse its connection-state and log mounting; extract a shared helper only if both editors would
otherwise copy more than a few lines).

src/socketio_editor.rs: init registers the NewSocketIoRequest handler and the SocketIo opener. Save
writes a .socketio.json item.

Tests (socketio_editor_tests.rs; test Socket.IO server): emit_with_ack_shows_linked_reply,
listen_list_edits_filter_live, new_socketio_request_opens_editor.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor socketio_editor::` → `PASS 3`.

### S65 — Sessions in the Console and History
**Goal:** every WebSocket and Socket.IO session leaves one Console entry (handshake, counts, close
reason) and one History row (protocol, target, duration, counts). Reopening the History row opens the
request.
**Depends on:** S51e, S63a, S63b, S64. **Deploy delta:** none.
**Edits:**
- `crates/request_editor/src/session_recording.rs`, `crates/request_editor/src/session_recording_tests.rs`;
- `crates/request_editor/src/websocket_editor.rs`;
- `crates/request_editor/src/socketio_editor.rs`;
- `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Read crates/console/src/session_entry.rs, crates/storage (HistoryEntry has protocol), plan D7
(later protocols get their own row shapes), crates/http_engine/src/history.rs.

src/session_recording.rs: feed each session's events to a SessionRecorder, and on Closed write one
HistoryEntry with protocol "websocket" or "socketio". No payloads in history; counts only.
websocket_editor.rs and socketio_editor.rs call it.

Tests (session_recording_tests.rs): websocket_session_writes_one_history_row,
socketio_session_console_entry_has_counts, history_never_contains_payloads.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor session_recording::` → `PASS 3`.

### S65w — Wire WebSocket and Socket.IO into the app (M4 E2E)
**Goal:** in the app as `pustman::init` builds it, `NewWebSocketRequest` connects to a local echo
server and the Message log shows the echo. `NewSocketIoRequest` opens a Socket.IO tab.
**Depends on:** S65, S63b. **Deploy delta:** none.
**Edits:** `crates/pustman/tests/wire_websocket.rs`, `crates/pustman/Cargo.toml` (hotspot; append dev-deps only).

```
Read crates/pustman/tests/wire_request_editor.rs. tests/wire_websocket.rs: init the app with the
in-memory store, dispatch NewWebSocketRequest, connect to websocket_client's test echo server, send
"hello", and assert a received row. Then dispatch NewSocketIoRequest and assert the tab opened.
Test: wire_websocket_echo_and_socketio_tab. Do not edit crates/pustman/src.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_websocket` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S66 — M4 ACs: echo server, Socket.IO server and a 100k-event log
**Goal:** the M4 exit criteria pass as automated tests.
**Depends on:** S65w. **Deploy delta:** none.
**Edits:** `crates/integration_tests/tests/m4_sessions.rs`, `crates/integration_tests/Cargo.toml` (append deps).

```
Read docs/PRD.md §11 M4, §5.4/§5.5 ACs, crates/websocket_client test servers,
crates/response_viewer session_store.rs and session_log.rs.

tests/m4_sessions.rs:
- websocket_echo_ac: connect with a subprotocol, send text, JSON and binary, receive the echoes,
  and close with 4000 "bye"; assert the log rows and Closed.
- socketio_ac: emit with an ack, receive the linked reply, and receive a named event.
- hundred_thousand_event_log_ac: push 100k events, including 3 over 1 MB, into a SessionStore
  rendered by SessionLog. Assert that only visible rows render, the inline byte total stays under
  the S51b cap, the spill files exist, and that clearing deletes them.
```

**Verify:**
- `scripts/pustman-v1-test.sh integration_tests websocket_echo_ac` → `PASS 1`.
- `scripts/pustman-v1-test.sh integration_tests socketio_ac` → `PASS 1`.
- `scripts/pustman-v1-test.sh integration_tests hundred_thousand_event_log_ac` → `PASS 1`.

### S67 — M4 SIGN-OFF (HUMAN)
**Goal:** the M4 exit criteria are proven on real machines.
**Depends on:** S66. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux and macOS.
- WebSocket: wss://echo.websocket.org (or a local one) with a subprotocol; text, JSON and binary
  from a file; saved message resend; ping/pong toggle; close code and reason; a refused upgrade
  shows its status and body; the Console handshake entry.
- Socket.IO: a local v4 server with namespace and auth; emit with ack; named listeners.
- Log: 100k events scroll smoothly; search, filter, pause, clear and export to JSONL.
Record M4 GO/NO-GO, then /promote → the next free release tag.
```

**Verify:** the log records `M4 GO`. CI is green at the merge SHA.

---

# Phase 9 — M5: gRPC

### S68a — Descriptors from `.proto` files and descriptor sets
**Goal:** `grpc_client` compiles `.proto` files with import paths in process (protox), or loads a
FileDescriptorSet, and lists services and methods with their streaming kind.
**Depends on:** S50a. **Deploy delta:** none.
**Edits:** `crates/grpc_client/src/descriptors.rs`, `crates/grpc_client/src/descriptors_tests.rs`,
`crates/grpc_client/src/grpc_client.rs` (append `mod` only), `crates/grpc_client/tests/fixtures/` (test protos).

```
Read docs/PRD.md REQ-GRPC-01/02, protox and prost-reflect via Context7. No build.rs, no protoc
(Phase 6 decision).

src/descriptors.rs:
- `load(source) -> Result<DescriptorPool, DescriptorError>` for ProtoFiles{files, import_paths}
  (protox) and DescriptorSet{path}. Errors name the file, line and column from protox;
- `methods(pool) -> Vec<MethodInfo{service, method, kind: Unary|ServerStreaming|ClientStreaming|
  Bidi, input, output}>`, sorted by service then method.
Fixtures: helloworld.proto (Greeter/SayHello) and a streaming.proto with all four kinds, importing
a common.proto through an import path.

Tests (descriptors_tests.rs): compiles_protos_with_import_path, lists_all_four_method_kinds,
syntax_error_names_file_and_line, loads_descriptor_set_file.
```

**Verify:** `scripts/pustman-v1-test.sh grpc_client descriptors::` → `PASS 4`.

### S68b — Server reflection and the in-process test server
**Goal:** descriptors load from server reflection (`grpc.reflection.v1`, falling back to `v1alpha`).
A server without reflection yields "Server reflection not available — import .proto files".
**Depends on:** S68a. **Deploy delta:** none.
**Edits:** `crates/grpc_client/src/reflection.rs`, `crates/grpc_client/src/reflection_tests.rs`,
`crates/grpc_client/src/test_server.rs`, `crates/grpc_client/src/grpc_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-GRPC-01 and the §5.6 AC, tonic and tonic-reflection via Context7.

src/reflection.rs: a client for the reflection service built from tonic-reflection's protobuf types.
It lists services and fetches file descriptors transitively into a DescriptorPool.
- Unimplemented on v1 retries v1alpha.
- Unimplemented on both maps to DescriptorError::ReflectionUnavailable, whose Display is exactly
  "Server reflection not available — import .proto files".
src/test_server.rs (cfg(any(test, feature = "test-support"))): a tonic server with no generated code.
- Services route by path through a tower Service using a DynamicMessage codec over the fixture
  pool.
- Implements Greeter.SayHello and the four streaming kinds.
- Reflection v1 is served via tonic-reflection's builder from the protox-encoded descriptor set,
  and it can start with reflection off.

Tests (reflection_tests.rs): reflection_v1_lists_greeter, falls_back_to_v1alpha,
no_reflection_reports_exact_message.
```

**Verify:** `scripts/pustman-v1-test.sh grpc_client reflection::` → `PASS 3`.

### S68c — Messages as JSON
**Goal:** request messages are edited as proto3 JSON. "Generate example message" fills every field
from the descriptor, and validation errors name the field path.
**Depends on:** S68a. **Deploy delta:** none.
**Edits:** `crates/grpc_client/src/message_json.rs`, `crates/grpc_client/src/message_json_tests.rs`,
`crates/grpc_client/src/grpc_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-GRPC-03, prost-reflect's serde support via Context7.

src/message_json.rs:
- `parse(json, descriptor) -> Result<DynamicMessage, MessageError{path, reason}>`;
- `to_json(&DynamicMessage) -> String` (proto3 JSON mapping, pretty);
- `example(descriptor) -> String`: every field set to a type-appropriate sample. Recursion stops
  at depth 3 for recursive types; enums take their first value.

Tests (message_json_tests.rs): example_message_parses_back, unknown_field_error_names_path,
wrong_type_error_names_nested_path, recursive_message_example_terminates.
```

**Verify:** `scripts/pustman-v1-test.sh grpc_client message_json::` → `PASS 4`.

### S68d — Unary calls
**Goal:** a unary call sends metadata (ASCII and `-bin`), auth as metadata, a deadline, over TLS or
plaintext h2c, with client certificates and CA. It returns the response, status name and number,
message, headers, trailers and decoded `google.rpc.Status` details.
**Depends on:** S68b, S68c, S52b. **Deploy delta:** none.
**Edits:** `crates/grpc_client/src/unary.rs`, `crates/grpc_client/src/unary_tests.rs`,
`crates/grpc_client/src/grpc_client.rs` (append `mod` only), `crates/grpc_client/Cargo.toml` (append deps).

```
Read docs/PRD.md REQ-GRPC-04/05, tonic (Channel, Endpoint, Grpc client with a custom codec),
tonic-types (Status details), crates/api_model/src/network.rs, crates/http_engine client_identity.rs
(reuse its PEM/PFX/CA loading through a pub fn; do not copy it).

src/unary.rs:
- `call_unary(&GrpcRequest, pool, network, secrets) -> Result<GrpcResponse{message_json, status:
  {code_name, code, message, details_json}, headers, trailers, elapsed}>`;
- metadata keys ending in -bin take base64 values;
- Bearer, Basic or API key become metadata;
- the deadline maps to grpc-timeout.
Never log metadata values that came from secrets.

Tests (unary_tests.rs; test_server): say_hello_returns_ok_0_with_headers_and_trailers,
deadline_exceeded_maps_status_4, binary_metadata_round_trips,
rich_status_details_decoded.
```

**Verify:** `scripts/pustman-v1-test.sh grpc_client unary::` → `PASS 4`.

### S68e — Streaming calls
**Goal:** server-streaming, client-streaming and bidirectional calls run as a `SessionEvent`
session: send messages, half-close, cancel. Each message carries its index, and the final status and
trailers end the session.
**Depends on:** S68d, S51a. **Deploy delta:** none.
**Edits:** `crates/grpc_client/src/streaming.rs`, `crates/grpc_client/src/streaming_tests.rs`,
`crates/grpc_client/src/grpc_client.rs` (append `mod` only).

```
Read docs/PRD.md REQ-GRPC-06, §9 (SessionEvent), crates/grpc_client/src/unary.rs.

src/streaming.rs: `start_stream(...) -> (GrpcStreamHandle{send, half_close, cancel}, stream of
SessionEvent)`.
- Messages map to ProtocolFields::Grpc{message_index}.
- Closed carries the status and trailers.
- Dropping the stream cancels the call.

Tests (streaming_tests.rs): server_streaming_receives_indexed_messages,
client_streaming_half_close_returns_response, bidi_interleaves_send_and_receive,
cancel_reports_cancelled_status.
```

**Verify:** `scripts/pustman-v1-test.sh grpc_client streaming::` → `PASS 4`.

### S69a — gRPC status panel
**Goal:** a gRPC response shows:
- the status name and number with a success or error colour, and the status message;
- the response headers and trailers as two tables;
- decoded status details.
**Depends on:** S59b, S68d. **Deploy delta:** none.
**Edits:** `crates/response_viewer/src/grpc_status.rs`, `crates/response_viewer/src/grpc_status_tests.rs`,
`crates/response_viewer/src/response_viewer.rs`, `crates/response_viewer/Cargo.toml` (append deps).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-GRPC-05, crates/response_viewer/src/status_line.rs,
headers_view.rs (reuse), response_viewer.rs.

src/grpc_status.rs renders a GrpcResponse: the body (message JSON via pretty.rs), Headers, Trailers,
and Status details. response_viewer.rs shows it for gRPC responses.

Tests (grpc_status_tests.rs): ok_status_shows_name_and_zero, error_status_shows_message_and_details,
trailers_listed_separately_from_headers.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer grpc_status::` → `PASS 3`.

### S69b — gRPC editor
**Goal:** a gRPC tab has:
- the target, a TLS toggle and a descriptor source (reflection, protos or descriptor set);
- a method picker with a kind badge;
- a JSON message editor with "Generate example message" and inline validation;
- metadata, auth and a deadline.
Invoke runs a unary call. `NewGrpcRequest` opens one, and History and the Console record protocol
`grpc`.
**Depends on:** S68d, S69a, S50d, S50e. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/grpc_editor.rs`, `crates/request_editor/src/grpc_editor_tests.rs`,
`crates/request_editor/src/request_editor.rs` (append `mod`/init lines only), `crates/request_editor/Cargo.toml` (append deps).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-GRPC-01..05, crates/request_editor/src/request_editor.rs,
key_value_table.rs (metadata), auth_tab.rs, open_item.rs, crates/grpc_client.

src/grpc_editor.rs:
- Descriptors load on a background task when the target or source changes. A reflection failure
  shows the exact message with an "Import .proto files" button (S69d).
- Invoke runs call_unary on a stored Task; Cancel drops it.
- History and Console record protocol "grpc" through the S51e recorder.
init registers the NewGrpcRequest handler and the Grpc opener. Save writes a .grpc.json item.

Tests (grpc_editor_tests.rs; test_server): reflection_fills_method_picker_with_kinds,
generate_example_fills_editor, invalid_message_shows_field_path, invoke_shows_ok_status.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor grpc_editor::` → `PASS 4`.

### S69c — gRPC streaming in the editor
**Goal:** for streaming methods, the editor shows the Message log with Send (client and bidi),
"End stream" (half-close) and Cancel. The final status shows when the call ends.
**Depends on:** S69b, S68e, S51d. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/grpc_streaming.rs`, `crates/request_editor/src/grpc_streaming_tests.rs`,
`crates/request_editor/src/grpc_editor.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read crates/request_editor/src/grpc_editor.rs,
crates/grpc_client/src/streaming.rs, crates/response_viewer session_log.rs.

src/grpc_streaming.rs: mounted by grpc_editor.rs when the method kind is not unary. The controls are
actions with palette names, and the session Task lives in the editor.

Tests (grpc_streaming_tests.rs): server_stream_rows_show_message_index,
client_stream_send_then_end_stream_shows_response, cancel_shows_cancelled_status.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor grpc_streaming::` → `PASS 3`.

### S69d — Import `.proto` files into the workspace
**Goal:** "Import .proto files" copies the chosen files into the workspace's `protos/` folder,
preserving relative paths. It sets the request's import paths, and the protos are then committed with
the workspace like any other file.
**Depends on:** S69b, S69c. **Deploy delta:** none.
**Edits:** `crates/request_editor/src/proto_import.rs`, `crates/request_editor/src/proto_import_tests.rs`,
`crates/request_editor/src/grpc_editor.rs`, `crates/request_editor/src/request_editor.rs` (append `mod` only).

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-GRPC-01, §6.1 (protos/), crates/workspace_store (atomic
writes; grep), crates/request_editor/src/grpc_editor.rs.

src/proto_import.rs:
- a file dialog (multi-select, .proto), then a copy on a background task with atomic writes;
- a name collision asks before overwriting;
- then switch the request's source to ProtoFiles with import_paths = ["protos"] and reload
  descriptors.
Failures show a toast naming the file.

Tests (proto_import_tests.rs): import_copies_into_protos_and_sets_import_paths,
collision_asks_before_overwrite, imported_protos_reload_method_list.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor proto_import::` → `PASS 3`.

### S69w — Wire gRPC into the app (M5 E2E)
**Goal:** in the app as `pustman::init` builds it, `NewGrpcRequest` opens a gRPC tab. Reflection
against the test server lists `helloworld.Greeter/SayHello`, and Invoke shows `OK (0)`.
**Depends on:** S69c, S69d. **Deploy delta:** none.
**Edits:** `crates/pustman/tests/wire_grpc.rs`, `crates/pustman/Cargo.toml` (hotspot; append dev-deps only).

```
Read crates/pustman/tests/wire_request_editor.rs. tests/wire_grpc.rs: init the app with the
in-memory store, start grpc_client's test server with reflection, dispatch NewGrpcRequest, set the
target, pick SayHello, generate an example, invoke, and assert "OK (0)". Test:
wire_grpc_reflection_and_unary_ok. Do not edit crates/pustman/src.
```

**Verify:**
- `scripts/pustman-v1-test.sh pustman wire_grpc` → `PASS 1`.
- `cargo build -p pustman` → exit 0.

### S70 — gRPC AC: reflection, proto import and all four call kinds
**Goal:** the PRD §5.6 AC and the M5 exit criterion pass as automated tests.
**Depends on:** S69w. **Deploy delta:** none.
**Edits:** `crates/integration_tests/tests/grpc_ac.rs`, `crates/integration_tests/Cargo.toml` (append deps).

```
Read docs/PRD.md §5.6 AC, §11 M5, crates/grpc_client (test_server, descriptors, unary, streaming).

tests/grpc_ac.rs:
- grpc_reflection_ac: reflection lists Greeter; SayHello returns OK (0) with headers and trailers.
- grpc_proto_import_ac: with reflection off, the exact message shows; after importing
  helloworld.proto, the same call succeeds.
- grpc_all_four_kinds_ac: unary, server, client and bidi streaming each complete with OK and the
  expected message counts.
```

**Verify:**
- `scripts/pustman-v1-test.sh integration_tests grpc_reflection_ac` → `PASS 1`.
- `scripts/pustman-v1-test.sh integration_tests grpc_proto_import_ac` → `PASS 1`.
- `scripts/pustman-v1-test.sh integration_tests grpc_all_four_kinds_ac` → `PASS 1`.

### S71 — M5 SIGN-OFF (HUMAN)
**Goal:** the M5 exit criteria are proven on real machines.
**Depends on:** S70. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux and macOS.
- A real gRPC server with reflection (e.g. grpcb.in): unary and streaming calls.
- A server without reflection, after importing its .proto files; the protos/ folder commits
  cleanly with Git.
- -bin metadata; a deadline exceeded; TLS with a client certificate; h2c to localhost.
Record M5 GO/NO-GO, then /promote → the next free release tag.
```

**Verify:** the log records `M5 GO`. CI is green at the merge SHA.

---

# Phase 10+ — Later milestones (STUBS: not dispatchable)

Each stub is expanded into full `S<N>` steps by a human-approved planning session, following the same
shape and appending after the last step. A driver must **never** dispatch a STUB row; reaching one is
a stop condition.

| Stub | Milestone | Crates | Depends on | Verify source (PRD) |
|---|---|---|---|---|
| STUB-M6 | MQTT | mqtt_client ★, request_editor | S51d, S50e | §5.7; pub/sub round trip v3.1.1 + v5 on a local broker. Adds an `Mqtt` `Request` variant (the only step editing `api_model/src/request.rs` then) and `.mqtt.json`. |
| STUB-M7 | v1 hardening: Git panel, GraphQL introspection/explorer/subscriptions (REQ-GQL-02/03/04), SSE raw view and reconnect (REQ-SSE-02/03), WS connection settings (REQ-WS-06), gRPC-Web (REQ-GRPC-07), Socket.IO v2/v3, OpenAPI import, HAR export and its redaction assertion, OAuth 1.0, NTLM, draft autosave/restore for non-HTTP tabs | git, collections_panel, console, api_model, request_editor | S60, S67, S71, STUB-M6 | §11 M7; v2.1 import corpus ≥ 99%; accessibility checklist |

STUB-M6 can be expanded and run in parallel with M3–M5 once Phase 6 has passed. It reuses the
SessionEvent model and the Message log, and needs no new root dependency step beyond declaring
`rumqttc`, which its expansion adds as its own first step.

## Notes on "standalone & verifiable"

- **Additive only.** Every step adds a crate, a module or a UI surface, and no earlier behavior is removed.
  There is no flag, because nothing reaches users until a milestone tag is built and smoked.
- **Rollback** is reverting the step's squash commit, or rebuilding the previous `release/pustman-v1-<n>` tag.
- A red Verify stops the step there. Never stack the next step on red.
- **Domain steps (S1–S10)** verify on this machine today. **`[GPUI]` steps** verify after G-DEPS, and in
  CI on GitHub (which installs the libraries) from S1 on.
