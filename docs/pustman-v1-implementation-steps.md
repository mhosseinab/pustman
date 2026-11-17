# Pustman v1 (M0–M2) — Step-by-Step Implementation

**Status: READY TO EXECUTE (2026-11-17).** Companion to [`pustman-v1-plan.md`](pustman-v1-plan.md)
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
  - **STUB-M3..M7**, the expansion gates
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

## Hotspots (the driver serializes them even when the graph says independent)

```
hotspots:
  - Cargo.lock                    # regenerate at merge, never hand-merged
  - crates/pustman/src/main.rs    # init order; touched by S22 S24 S26 S29 S32 S34 S35 S39 S44 (9 of 49 = 18%)
  - crates/pustman/Cargo.toml     # same steps as main.rs
```

These are under the 25% gate, so no refactor-for-parallelism step is needed. Two design choices keep
other files off this list:

- The root `Cargo.toml` uses `members = ["crates/*"]` and declares every dependency in S1.
- Keymaps are written once in S21.

## Dependency graph (quick view)

`A→B` = B depends on A. Steps on separate lines with no edge between them run in parallel.
`⟂G` = waits for human gate G-DEPS.

```
Phase 0 harness        S0
Phase 1 domain         S0→S1 ; S1→S2→S3→S9→S10 ; S2→S4 ; S2→S5→S8 ; S1→S6 ; S1→S7
                       ── G-DEPS (human: install GPUI libs, push for CI) ── promote release/pustman-v1-1 ──
Phase 2 http_engine ⟂G S2,S4→S11→S12→S13→S14→S15→S16   (S13 also needs S7; S15 needs S6; S16 needs S3,S6)
Phase 3 shell ⟂G       S1→S17→S18→S19→S20 ; S1→S21 ; S6,S19,S21→S22→S23 ; S20,S22→S24
                       S23,S24→S25 (M0 SIGN-OFF, human) ── promote -2
Phase 4 HTTP UI        S16,S24,S25→S26→S27→S28→S29→S30→S31→S36
                       S22,S16,S25→S32 ; S28,S29→S33 ; S20,S22,S4,S7,S25→S34 ; S26→S35
                       S5,S16,S32→S37
                       S31,S32,S33,S34,S35,S36,S37→S38 (M1 SIGN-OFF, human) ── promote -3
Phase 5 workspaces     S38→S39→S40 ; S23,S39→S41 ; S8,S40,S41→S42 ; S36,S38→S43
                       S41→S44 ; S9,S42→S45 ; S41→S46 ; S37,S43,S44→S47
                       S42,S43,S45,S46,S47→S48 (M2 SIGN-OFF, human) ── promote -4
Phase 6+ (stubs)       S48→STUB-M3 ; S48→STUB-M4 ; S48→STUB-M5 ; S48→STUB-M6 ; STUB-M3..M6→STUB-M7
```

The serial chains are deliberate:
- S3→S9→S10 share `crates/api_model/src/api_model.rs`.
- S11–S16 share `crates/http_engine/src/http_engine.rs`.
- S26→S28→S29→S33 share `crates/request_editor/**` (S29 also edits it, so S33 waits for S29).
- S29→S31 and S36 share `crates/response_viewer/**`.

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

### S13 — Send pipeline: variable resolution, auth (Basic/Bearer/API key), unresolved secrets
**Goal:** `HttpRequest` + scopes + secrets become a `ResolvedRequest`:
- Effective auth is applied.
- Unresolved variables are reported.
- A secret with no local value fails as `UnresolvedSecret` before any network call.

**Depends on:** S12, S7.
**Deploy delta:** none.
**Edits:** `crates/http_engine/**`.

```
Read crates/http_engine, crates/variables, crates/secrets, crates/api_model/src/auth.rs, docs/PRD.md
REQ-VAR-01/02/07, REQ-MODEL-01, REQ-HTTP-06 (M1 subset only: Inherit, None, Basic, Bearer, API key
in header or query), §8.6 UnresolvedSecret. Add pipeline.rs: `prepare(request, ancestors: &[&Auth],
scopes, secrets: &dyn SecretStore, dynamic: &mut DynamicValues) -> Result<Prepared, TransportError>`
where Prepared = { resolved: ResolvedRequest, warnings: Vec<Warning> (UnresolvedVariables(names),
TlsVerifyDisabled), secrets_used: SecretValues (for redaction downstream) }. Resolve URL, params,
headers, body text, auth fields; one DynamicValues per send. Secret with no local value →
Err(UnresolvedSecret{names}) without touching the transport. Wire HttpClient::send to call
prepare then the transport. Do not log any value. Tests: resolves_url_headers_and_body,
basic_auth_header_applied, bearer_inherited_from_collection, api_key_in_query,
unresolved_variable_warns_but_sends, missing_secret_fails_before_network (assert FakeTransport saw
zero calls), dynamic_guid_same_across_fields_in_one_send.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine pipeline` → `PASS 7`.

### S14 — Request bodies, file refs, auto headers, large response spill
**Goal:**
- Request bodies:
  - All M1 body modes send correctly.
  - File references resolve relative to the workspace root, and a missing file gives `FileNotFound`.
  - Auto headers are added and listed.
- Large responses: anything over 10 MB keeps an in-memory prefix and spills to a temp file.

**Depends on:** S13. **Deploy delta:** none.
**Edits:** `crates/http_engine/**`.

```
Read crates/http_engine, docs/PRD.md REQ-HTTP-03/04/05, REQ-RESP-06, REQ-NFR-03. Add body.rs:
encode Body modes (form-data with text + file fields and per-field content type via reqwest
multipart; x-www-form-urlencoded; raw with Content-Type from language unless the user set one;
binary file) and auto headers (Content-Type, Content-Length, User-Agent "Pustman/<version>",
Accept "*/*", Accept-Encoding, Host) listed in `ResolvedRequest.auto_headers` so the UI can show
them; a user header with the same name wins. File refs: relative → workspace_root.join; absolute
kept; missing → FileNotFound{path}. Responses stream: keep the first 10 MB (configurable via
RequestSettings / global default) in memory as `ResponseBody::Prefix` and stream the rest to a
tempfile in the OS temp dir → `ResponseBody::Spilled{prefix, path, total_len}`; temp files deleted
on drop of the owning handle. Tests: json_raw_body_sets_content_type,
user_content_type_overrides_auto, urlencoded_body_encodes_pairs, form_data_file_field_sent,
relative_file_ref_resolves_from_workspace_root, missing_file_is_file_not_found_error,
response_over_limit_spills_to_temp_file, temp_file_removed_on_drop.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine body` → `PASS 8`.

### S15 — Cookie jar per workspace
**Goal:** cookies are captured from responses, applied to matching requests, and persisted per
workspace through `storage`. Workspace files never hold them.
**Depends on:** S14, S6. **Deploy delta:** none.
**Edits:** `crates/http_engine/**`.

```
Read docs/PRD.md REQ-HTTP-08, REQ-WSP-06, crates/storage (cookies table), crates/http_engine. Add
cookies.rs: a per-workspace `CookieJar` (cookie_store crate) applied manually in ReqwestTransport
(so redirect hops capture/apply cookies too); load from / save to storage on a background task
(never on the foreground); API for the Manage Cookies UI: list, add, edit, delete, clear by
domain. The applied Cookie header appears in the resolved request (REQ-NET-04). Tests:
set_cookie_captured_and_sent_on_next_request, cookie_scoped_by_domain_and_path,
redirect_hop_cookie_applied, jar_persists_and_reloads_from_storage, delete_cookie_removes_it.
```

**Verify:** `scripts/pustman-v1-test.sh http_engine cookie` → `PASS 5`.

### S16 — History recording with redaction, 1 MB cap, retention
**Goal:** every send writes a history row with redacted, pre-resolution request data. The response
summary is stored and bodies are capped at 1 MB. Retention prunes old rows.
**Depends on:** S15, S3, S6. **Deploy delta:** none.
**Edits:** `crates/http_engine/**`.

```
Read docs/PRD.md §8.5 (REQ-HIST-01..04), REQ-NFR-04, crates/api_model/src/redact.rs, crates/storage
history repo. Add history.rs: after each send (success, error or cancel) build a HistoryRecord from
the PRE-resolution HttpRequest (so {{vars}} stay as written) + response summary (status, timings,
sizes, headers redacted) + body prefix ≤ 1 MB; run redact on every text field with the send's
secrets_used plus header rules; write on a background task; prune by the retention setting
(default 30 days) at most once per app session. Tests: history_stores_pre_resolution_request,
history_never_contains_secret_value (scan the whole SQLite file bytes for the secret),
history_body_capped_at_one_megabyte, cancelled_send_recorded_as_cancelled,
retention_prunes_entries_older_than_setting.
```

**Verify:**
- `scripts/pustman-v1-test.sh http_engine history` → `PASS 5`.
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

### S19 — `ui` design-system crate: tokens and base components
**Goal:** a prelude of our own components that every later UI step uses:
- Button, IconButton, Label, Icon, Tab/TabBar, Table, Toggle, Tooltip, ContextMenu, Modal frame,
  Toast, Badge, Divider, List with virtualization

**Depends on:** S18. **Deploy delta:** none.
**Edits:**
- `crates/ui/**`
- `assets/icons/**`

```
Load the rust-gpui-ui-ux skill (components, spacing, accessibility references). Read docs/PRD.md §3,
REQ-UI-07, REQ-NFR-06. Build crates/ui with a `prelude` module; every component is RenderOnce or
an Entity as the skill prescribes, reads colors only from cx.theme(), takes a stable ElementId,
is keyboard-focusable where interactive, and has an accessible label. Icons: 16x16 SVGs
(Lucide-derived, ISC license noted in assets/icons/LICENSE) for the set PRD M0–M2 needs; embed via
an AssetSource. Do NOT generate raster images. Include a `story`-style test view rendering every
component. Tests (#[gpui::test] with VisualTestContext): all_components_render_in_both_themes,
button_activates_on_enter_and_space, tab_bar_cycles_with_keyboard, toast_auto_dismisses_after_timeout
(GPUI timer), virtual_list_renders_only_visible_rows.
Report which ux-checklist.md items you verified.
```

**Verify:** `scripts/pustman-v1-test.sh ui` → `PASS 5`.

### S20 — `text_input` crate: single- and multi-line editing
**Goal:** text fields support typing, selection, clipboard, undo and IME through
`EntityInputHandler`. They expose highlight ranges for variable spans.
**Depends on:** S19. **Deploy delta:** none.
**Edits:** `crates/text_input/**`.

```
Load the rust-gpui-ui-ux skill (text input reference) and read GPUI's examples/input.rs at the
pinned rev (Apache — may be adapted). Build TextInput (single-line) and TextArea (multi-line,
line numbers optional, soft wrap) entities: cursor movement by char/word/line, selection with
shift, select-all, copy/cut/paste, undo/redo, IME via EntityInputHandler, placeholder, read-only
mode, `set_highlights(Vec<(Range<usize>, HighlightStyle)>)` for variable spans, emits
`Changed`/`Submitted` events. Typing must not reallocate the whole buffer per keystroke for large
bodies (use a rope or gap buffer; record a Decision). Tests: typing_inserts_at_cursor,
word_motion_and_shift_selection, undo_redo_restores_text, paste_replaces_selection,
highlights_survive_edits_before_them, enter_in_single_line_emits_submitted.
```

**Verify:** `scripts/pustman-v1-test.sh text_input` → `PASS 6`.

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

### S22 — `pustman` binary + `workspace` shell (window, docks, panes/tabs, status bar, toasts)
**Goal:** `cargo run -p pustman` opens a window with:
- a left dock
- a center pane with tabs
- a bottom dock
- a status bar
- toasts

Startup time is traced.
**Depends on:** S6, S19, S21. **Deploy delta:** none.
**Edits:**
- `crates/pustman/**` (hotspot)
- `crates/workspace/**`

```
Load rust-gpui-ui-ux and pustman-feature-crate skills. Read AGENTS.md (init order), docs/PRD.md §3
(REQ-UI-01/02/04/07), REQ-NFR-02. Write our own workspace crate (Zed's is GPL — patterns only):
Workspace entity with left Dock, center PaneGroup (single Pane with tabs is enough for M0;
reorder, close, pin, dirty dot), bottom Dock, StatusBar with `add_left_item/add_right_item`,
`Panel` trait so feature crates register their own panels (no edits to workspace for each
panel), `Item` trait for tab contents, ModalLayer, toast notifications (`workspace.show_toast`).
A placeholder "Welcome" item is allowed as the empty-state view (it is real UI, not a stub).
Handlers for NewTab/CloseTab/NextTab/PreviousTab/ToggleSidebar/ToggleConsole. crates/pustman:
main.rs initializes settings → theme → ui → pustman_actions keymap → storage (open LocalStore in
the platform data dir on a background task; failure → toast and in-memory store) → workspace, opens
the window. With PUSTMAN_STARTUP_TRACE=1 print `startup_ms=<n>` to stderr at the first frame
painted. Tests (#[gpui::test]): workspace_opens_with_docks_and_status_bar,
close_tab_action_closes_active_tab, next_tab_cycles, toggle_sidebar_hides_left_dock,
toast_shows_and_dismisses, panel_registration_adds_dock_entry.
```

**Verify:**
- `scripts/pustman-v1-test.sh workspace` → `PASS 6`.
- `cargo build -p pustman` → exit 0.
- E2E (display): `PUSTMAN_STARTUP_TRACE=1 cargo run --release -p pustman` prints `startup_ms=<n>`, and the window shows the docks and status bar.

### S23 — Tab and layout restore
**Goal:** open tabs (with unsaved drafts), dock sizes and visibility are restored after a restart.
Drafts autosave every 2 s.
**Depends on:** S22. **Deploy delta:** none.
**Edits:** `crates/workspace/**`.

```
Read docs/PRD.md REQ-UI-01/02, REQ-NFR-05, crates/workspace, crates/storage (open_tabs, layout).
Serialize layout (dock sizes/visibility) and open tabs (item ids, order, pinned, active,
draft_json via an `Item::serialize_draft` hook) to storage on change, debounced, on a background
task; restore on window open; autosave drafts every 2 s using a GPUI timer task stored on the
Workspace. Items whose backing file vanished restore as closed and a toast says so. Tests:
layout_restored_after_reopen, tabs_restored_in_order_with_active_and_pinned,
draft_autosaved_after_two_seconds (advance the test clock), missing_item_reported_not_restored.
```

**Verify:** `scripts/pustman-v1-test.sh workspace restore` → `PASS 4` (filter matches the 4 new tests).

### S24 — Command palette
**Goal:** `Ctrl/Cmd-Shift-P` opens a fuzzy palette listing every available action with its keybinding.
Choosing an entry dispatches the action.
**Depends on:** S20, S22. **Deploy delta:** none.
**Edits:**
- `crates/command_palette/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux and gpui-actions-keymaps. Read docs/PRD.md REQ-UI-05. Write our own Picker
(Zed's picker is GPL — patterns only) inside crates/command_palette (or crates/ui if generic —
record a Decision): modal with TextInput query, fuzzy-ranked list (own simple scorer), keyboard
up/down/enter/escape, shows the binding for each action from the active keymap. Register init in
main.rs. Tests: palette_lists_every_registered_action_with_binding, fuzzy_query_ranks_prefix_first,
enter_dispatches_selected_action, escape_dismisses.
```

**Verify:** `scripts/pustman-v1-test.sh command_palette` → `PASS 4`.

### S25 — M0 SIGN-OFF (HUMAN)
**Goal:** the M0 exit criteria are proven on real machines.
**Depends on:** S23, S24. **Deploy delta:** none. **Edits:** none.

```
Human validation, not code. On Linux (X11 and Wayland if available) and macOS:
1) `PUSTMAN_STARTUP_TRACE=1 cargo run --release -p pustman` 5 times → median startup_ms < 1000.
2) Open 3 tabs, resize/hide docks, quit, relaunch → tabs, active tab, dock sizes restored.
3) Palette lists every action with its binding; Ctrl-T/W/Tab, Ctrl-\, Ctrl-Alt-C work.
4) `cargo test -p secrets -- --ignored` passes (live keychain).
5) Theme follows OS appearance. Record ux-checklist items verified.
Record results + go/no-go in the S25 step record via the driver. Then /promote → release/pustman-v1-2.
```

**Verify:** the human records `M0 GO` with the 5 results in the log. CI is green on Linux and macOS at the S24 merge SHA.

---

# Phase 4 — HTTP UI + debugging (M1) [GPUI]

### S26 — `request_editor`: URL bar, method, send/cancel, response status line
**Goal:** a request tab shows a method picker and URL input. `Ctrl-Enter` sends through
`http_engine` and `Esc` cancels. A status line shows status, time and size, or the error.
**Depends on:** S16, S24, S25. **Deploy delta:** none.
**Edits:**
- `crates/request_editor/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-HTTP-01/12, REQ-UI-02/03, REQ-RESP-01, crates/http_engine,
crates/workspace (Item trait). RequestEditor Item: method dropdown (standard verbs + custom
token, validated), URL TextInput, Send/Cancel button that toggles while in flight, dirty dot via
Item; handlers for request::Send, request::Cancel (Esc), request::FocusUrlBar (Ctrl-L),
workspace::NewRequest. Holds `pending_send: Option<Task<()>>`; send failures render an inline error
(not only logs). Status line: status code colored by class, reason, total time, size. Register
init in main.rs. Tests: ctrl_enter_sends_via_fake_transport, esc_cancels_in_flight_send,
send_button_toggles_to_cancel_while_in_flight, error_shows_inline_message_and_fix,
custom_method_rejects_invalid_token, new_request_action_opens_tab.
Report ux-checklist items verified.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor` → `PASS 6`.
- E2E: `cargo run -p pustman`, then New Request, `GET https://example.com`, then `Ctrl-Enter`. The status line shows 200 and the time.

### S27 — Params, path variables, headers tables; cURL paste/copy
**Goal:**
- Params:
  - URL query and the Params table stay in sync both ways.
  - `:path` variables get rows.
- Headers:
  - The table shows auto headers (read-only).
- cURL:
  - Pasting a cURL command into the URL bar imports it.
  - Copy-as-cURL works.

**Depends on:** S26, S10. **Deploy delta:** none.
**Edits:** `crates/request_editor/**`.

```
Read docs/PRD.md REQ-HTTP-02/03/13, crates/api_model/src/curl.rs, crates/ui Table. Add a reusable
KeyValueTable (key, value, description, enabled; add/remove rows; keyboard navigable) in
request_editor; Params tab synced both ways with the URL (edit either side, no feedback loop);
path variables from `:name` segments; Headers tab with header-name autocomplete from a static
common-headers list and auto headers shown greyed with a "auto" badge (from
ResolvedRequest.auto_headers via a dry prepare). Paste into the URL bar of text starting with
`curl ` → parse_curl → replace the request (undoable), parse error → inline error. request::CopyAsCurl
copies to_curl output. Tests: editing_url_updates_params_table, editing_params_updates_url,
path_variable_rows_follow_url, auto_headers_listed_read_only, pasting_curl_imports_request,
copy_as_curl_writes_clipboard.
```

**Verify:** `scripts/pustman-v1-test.sh request_editor` → `PASS n` (n ≥ 12).

### S28 — Body, auth and settings tabs (TLS badge)
**Goal:** all M1 body modes and auth types are editable, and the effective auth source is shown.
Per-request settings are editable, and disabling TLS verification shows a persistent warning badge.
**Depends on:** S27. **Deploy delta:** none.
**Edits:** `crates/request_editor/**`.

```
Read docs/PRD.md REQ-HTTP-04/05/06/09/10, REQ-MODEL-01/03, REQ-VAR-SHARE-03, and the HTTP AC about
self-signed TLS. Body tab: mode switcher (none, form-data with file picker, urlencoded, raw with
language, binary); file paths stored relative to the workspace root when inside it. Auth tab:
Inherit/None/Basic/Bearer/API key; shows "Inherited from <collection/folder>" via effective_auth;
typing a literal token into a shared field shows the REQ-VAR-SHARE-03 hint suggesting a secret
variable. Settings tab: each RequestSettings field with "use global default" state. Disabling
verify_tls shows a warning badge next to Send while active. When a send fails with Tls, the error
offers a "Disable verification for this request" button that sets the override and re-sends.
Tests: body_mode_switch_preserves_other_modes_data, file_path_saved_relative_to_workspace,
auth_tab_shows_inherited_source, literal_token_shows_secret_hint,
tls_error_offers_disable_and_resend, tls_badge_visible_while_verify_disabled.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor` → `PASS n` (n ≥ 18).
- E2E: run `openssl s_server` on a self-signed cert at `https://localhost:8443`. The first send shows the TLS error. "Disable verification" makes it succeed, and the badge stays visible.

### S29 — `response_viewer`: Pretty/Raw body, headers, error panel
**Goal:** the response pane shows:
- the body as Pretty (JSON/XML/HTML formatted) or Raw
- a headers table that keeps duplicates in order
- an error panel with type, message, suggested fix and a link to the Console

**Depends on:** S28.
**Deploy delta:** none.
**Edits:**
- `crates/response_viewer/**`
- `crates/request_editor/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-RESP-01/02/05 (headers part)/08, §8.6. Create
crates/response_viewer: ResponseView entity (status line moved here from request_editor), tabs
Body/Headers (Cookies/Timeline come in S30/S31), Body sub-modes Pretty (JSON pretty-print,
XML/HTML indent; auto-detected from Content-Type with manual override) and Raw; virtualized line
list for large bodies; error panel for TransportError with user_message/suggested_fix and an
"Open in Console" link (dispatches console::Focus once it exists — until then the link is
hidden, not stubbed). Embed ResponseView under the request editor (split, resizable). Register
init in main.rs. Tests: json_body_pretty_printed, raw_mode_shows_exact_bytes,
content_type_override_switches_formatter, duplicate_headers_kept_in_order,
transport_error_renders_panel_with_fix, status_class_colors_from_theme.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer` → `PASS 6`.

### S30 — Response: Hex, Preview, Find, save/copy, large bodies, Cookies tab
**Goal:**
- Body views: Hex and safe Preview views.
- Find: `Ctrl-F` with regex and case options.
- Save and copy: the body saves to a file and copies, and a header can be copied.
- Large bodies: Pretty falls back to Raw above the limit.
- Cookies tab: lists the response's cookies.

**Depends on:** S29. **Deploy delta:** none.
**Edits:** `crates/response_viewer/**`.

```
Read docs/PRD.md REQ-RESP-02/03/05/06/07. Hex view (offset | hex | ascii, virtualized); Preview:
images rendered with GPUI img from bytes, HTML shown as sanitized plain text with a note (no
script execution, no network fetch); Find bar (regex, case toggle, next/prev, match count);
response::SaveBody (file dialog, write on background task, error → toast), CopyBody, copy single
header from the context menu; bodies over `response_pretty_limit_mb` (setting) show Raw with a
banner and offer "open temp file location"; Cookies tab lists Set-Cookie parsed with all
attributes. Tests: hex_view_formats_offsets, find_regex_counts_matches, find_case_sensitive_toggle,
save_body_writes_file, large_body_falls_back_to_raw_with_banner, cookies_tab_lists_attributes,
html_preview_does_not_execute_or_fetch.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer` → `PASS n` (n ≥ 13).

### S31 — Timeline tab: phases, network info, redirect chain, resolved request as sent
**Goal:** the Timeline shows:
- measured timing phases, with omitted ones absent rather than zeroed
- network/TLS info
- each redirect hop
- the exact request as sent, with secrets masked

**Depends on:** S30.
**Deploy delta:** none.
**Edits:** `crates/response_viewer/**`.

```
Read docs/PRD.md §8.2 (REQ-NET-01..04), docs/decisions/ADR-0003-timing-phases.md, crates/http_engine
Response/Timings/NetworkInfo/RedirectHop. Timeline tab: horizontal bars only for phases that are
Some; a note listing phases this transport cannot measure (from ADR-0003); network info section;
redirect chain list (method, URL, status, Location, timing; expandable headers); "Request as
sent": final URL, method, every header incl. auto/auth/cookie headers, body preview — passed
through api_model::redact with the send's secrets. Tests: absent_phases_not_rendered,
redirect_chain_lists_every_hop, request_as_sent_includes_auto_and_cookie_headers,
request_as_sent_masks_secrets_and_authorization.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer timeline` → `PASS 4`.

### S32 — `console` crate (bottom dock)
**Goal:** every send adds a Console entry with a summary line and expandable raw request/response
text. The Console supports filters, clear and pause, and is capped at 5,000 entries in memory.
Warnings land in it.
**Depends on:** S16, S22, S25. **Deploy delta:** none.
**Edits:**
- `crates/console/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md §8.3 (REQ-CON-01..04, 06), REQ-HTTP-12 (Cancelled entry),
REQ-NFR-04. Create crates/console: ConsoleStore Global (ring buffer, cap from settings
console_max_entries, memory only, never written to disk), fed by http_engine SendEvents
(subscribe; add a small event hook in http_engine ONLY if missing — if so, list it as a Decision
and ask the driver, because http_engine is outside this Edits set); entries: summary line (method,
URL, status, time, size), level, protocol, raw request/response text exactly as on the wire
(HTTP/1.1 form; HTTP/2 as pseudo-headers), redacted with api_model::redact. Console panel registers
in the bottom dock via the Panel trait; filters (level, protocol, text), clear, pause; toggled by
Ctrl-Alt-C. Warnings (unresolved vars, TLS disabled, missing body file, script not executed)
appear as warning entries. `export_text(entries) -> String` (redacted) for the S37 test. Tests:
send_creates_entry_with_summary, raw_view_shows_resolved_request_exactly,
cap_drops_oldest_entries, filters_by_level_and_text, pause_stops_new_entries,
tls_disabled_warning_logged, export_text_contains_no_secret.
```

**Verify:**
- `scripts/pustman-v1-test.sh console` → `PASS 7`.
- E2E: send `POST {{base}}/users` with the JSON body `{"name":"{{name}}"}` to a local echo server. The Console raw view shows the resolved URL, the body and `Content-Type: application/json`. This is the first PRD HTTP AC.

### S33 — Variable highlighting, autocomplete, hover, pre-send warning
**Goal:** in every request field:
- `{{var}}` is highlighted green (resolved), red (unresolved) or masked (secret).
- Variable names autocomplete after `{{`.
- Hover shows the value and its scope.
- Sending with unresolved variables shows a non-blocking warning.

**Depends on:** S28, S29 (both edit `crates/request_editor/**`). **Deploy delta:** none.
**Edits:**
- `crates/request_editor/**`
- `crates/text_input/**`

```
Read docs/PRD.md REQ-VAR-03/04/07, crates/variables (spans), crates/text_input (set_highlights).
Wire variables::spans into URL, params, headers, body raw and auth fields; autocomplete popover
after `{{` listing names across scopes in effect (keyboard navigable); hover tooltip shows the
value (masked for secrets, with a reveal toggle) and the scope it came from; before send, if
prepare() returns UnresolvedVariables, show a non-blocking warning bar listing them with "Send
anyway" (also logged to Console). text_input changes limited to what highlighting/autocomplete
need. Tests: resolved_unresolved_secret_spans_highlighted, autocomplete_lists_names_after_braces,
autocomplete_inserts_selected_name, hover_masks_secret_value, unresolved_warning_lists_names_and_allows_send.
```

**Verify:**
- `scripts/pustman-v1-test.sh request_editor variable` → `PASS 5`.
- `scripts/pustman-v1-test.sh text_input` → `PASS n` (n ≥ 6).

### S34 — `environment_picker` + quick-look
**Goal:** the title-bar picker selects the active environment, which is persisted per workspace in
local state. The quick-look popover lists environment and global values, with secrets masked.
**Depends on:** S4, S7, S20, S22, S25. **Deploy delta:** none.
**Edits:**
- `crates/environment_picker/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-VAR-08, REQ-VAR-SHARE-01/02. Picker (reuse the
command_palette picker if it was made generic, else a small dropdown) in the title bar listing
environments + "No environment"; selection stored in storage kv per workspace; quick-look popover
listing active env and globals: key, shared value, local value, secret masked (reveal toggle
reads the keychain on a background task); unset secrets labelled "unset". Register init.
Tests: selecting_environment_persists_per_workspace, quick_look_masks_secrets,
unset_secret_labelled_unset, no_environment_option_clears_selection.
```

**Verify:** `scripts/pustman-v1-test.sh environment_picker` → `PASS 4`.

### S35 — `history_panel`
**Goal:** the left-dock History panel groups sends by day and supports search. Opening an entry
restores the pre-resolution request and the response summary. Entries can be deleted or cleared,
and retention is honored.
**Depends on:** S26. **Deploy delta:** none.
**Edits:**
- `crates/history_panel/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md §8.5 (REQ-HIST-01..04), crates/storage history repo,
crates/request_editor. Panel registered in the left dock via the Panel trait; virtualized list
grouped by day; search box; open entry → new request tab with the stored pre-resolution request
and a read-only response summary; delete entry, clear all (confirm modal); "Save to collection"
is deferred to M2 (hidden, not stubbed). All DB access on background tasks; errors → toast.
Tests: entries_grouped_by_day, search_filters_entries, open_entry_restores_unresolved_request,
delete_and_clear_all_remove_entries.
```

**Verify:** `scripts/pustman-v1-test.sh history_panel` → `PASS 4`.

### S36 — Manage Cookies dialog
**Goal:** a dialog lets the user view, add, edit and delete the workspace cookie jar. It opens from
the response Cookies tab and from the `cookies::Manage` action.
**Depends on:** S31, S15. **Deploy delta:** none.
**Edits:** `crates/response_viewer/**`.

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-HTTP-08 and crates/http_engine/src/cookies.rs. Modal
(workspace ModalLayer) grouped by domain; add/edit form with validation; delete one / clear domain;
changes persist through the CookieJar API on a background task; errors → inline. Tests:
dialog_lists_cookies_by_domain, add_cookie_persists, edit_cookie_updates_value,
delete_cookie_removes_row.
```

**Verify:** `scripts/pustman-v1-test.sh response_viewer cookie` → `PASS n` (n ≥ 4).

### S37 — Redaction NFR test (integration)
**Goal:** one test proves that no secret value reaches workspace files, the SQLite file bytes, or
Console export text.
**Depends on:** S5, S16, S32. **Deploy delta:** none.
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

**Verify:** `scripts/pustman-v1-test.sh integration_tests redaction` → `PASS 2`.

### S38 — M1 SIGN-OFF (HUMAN)
**Goal:** the M1 exit criteria are proven: the PRD HTTP ACs pass, the redaction test passes, and the manual run-through is done.
**Depends on:** S31, S32, S33, S34, S35, S36, S37. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux + macOS: the four PRD §5.1 HTTP ACs by hand (echo server; a 6-hop
redirect server with max 5; openssl s_server self-signed; cancel immediately after send — Console
shows Cancelled, no retry); variables highlighting/autocomplete; history open/restore; cookies
dialog; cURL paste/copy; Timeline on a real HTTPS host. Confirm `scripts/pustman-v1-test.sh
integration_tests redaction` → PASS 2 on the merge SHA. Record M1 GO/NO-GO; then /promote →
release/pustman-v1-3.
```

**Verify:** the log records `M1 GO` and the four ACs by name. CI is green at the merge SHA.

---

# Phase 5 — Workspaces + sharing (M2) [GPUI]

### S39 — `collections_panel`: tree
**Goal:** the left dock shows the open workspace's collections, folders and requests as a
virtualized, keyboard-navigable tree. Opening a request opens its tab.
**Depends on:** S38. **Deploy delta:** none.
**Edits:**
- `crates/collections_panel/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Load rust-gpui-ui-ux. Read docs/PRD.md REQ-UI-01, REQ-NFR-02 (10,000-item tree at 60 fps),
crates/workspace_store (snapshot), crates/request_editor. Panel in the left dock: tree from the
WorkspaceSnapshot honoring `order`; expand/collapse; arrow-key navigation; Enter opens the request
tab (one tab per item id; re-opening focuses it); method badges; virtualized (uniform_list);
element ids from ItemId. Problems from the snapshot show an error badge (detail UI is S42). Tests:
tree_follows_order_arrays, keyboard_navigation_expands_and_opens, opening_twice_focuses_existing_tab,
ten_thousand_items_render_only_visible_rows.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel` → `PASS 4`.

### S40 — Collection CRUD, save, ordering, dirty state
**Goal:** the user can create, rename, duplicate, delete and drag-reorder collections, folders and
requests. `Ctrl-S` saves the active request to its file through `workspace_store`, and the dirty dot
clears.
**Depends on:** S39, S33. **Deploy delta:** none.
**Edits:**
- `crates/collections_panel/**`
- `crates/request_editor/**`

```
Read docs/PRD.md REQ-WSP-01..03, REQ-UI-02/03, crates/workspace_store save_item. Context menu +
actions: new collection/folder/request, rename (inline), duplicate, delete (confirm modal), drag
to reorder/move (updates the parent's order array). workspace::Save in the request editor calls
save_item on a background task; success clears the dirty dot; failure → toast with the path and
reason. Deleting removes the file(s). Tests: create_request_writes_file_and_appears_in_tree,
rename_keeps_id_and_updates_slug, reorder_updates_parent_order_array, delete_removes_file_after_confirm,
ctrl_s_saves_and_clears_dirty, save_failure_shows_toast.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel` → `PASS n` (n ≥ 10); `scripts/pustman-v1-test.sh request_editor save` → `PASS n` (n ≥ 2).

### S41 — Open, create and recent workspaces; format-version banner
**Goal:** the user can open a folder as a workspace, create a new one, or pick from the recent list.
The last workspace reopens at launch. A newer-major format opens read-only with a banner.
**Depends on:** S23, S39. **Deploy delta:** none.
**Edits:** `crates/workspace/**`.

```
Read docs/PRD.md REQ-WSP-04/05, REQ-COLLAB-06, crates/workspace_store open, crates/storage
recent_workspaces. Actions OpenWorkspace (folder dialog), CreateWorkspace (writes pustman.json +
layout; inside a git repo, show the REQ-COLLAB-06 note that no local files are stored there),
OpenRecent (picker); one workspace per window; last-opened workspace reopens at launch; read_only
snapshot → banner + editing disabled (save actions no-op with a toast). Errors → toast. Tests:
create_workspace_writes_pustman_json, open_workspace_loads_snapshot_and_records_recent,
newer_major_opens_read_only_with_banner, recent_list_most_recent_first.
```

**Verify:** `scripts/pustman-v1-test.sh workspace workspace_` → `PASS n` (n ≥ 4).

### S42 — External change bar and problem badges
**Goal:** watcher events update the tree within 1 s. An open, dirty tab whose file changed shows a
"Changed on disk — Reload / Keep mine / Compare" bar. Conflict or invalid JSON files show an error
badge that names the file and line.
**Depends on:** S8, S40, S41. **Deploy delta:** none.
**Edits:**
- `crates/collections_panel/**`
- `crates/request_editor/**`

```
Read docs/PRD.md REQ-COLLAB-01/02, crates/workspace_store watcher. Start the watcher when a
workspace opens (store the subscription/task); apply FsChange to the snapshot and tree; clean
open tabs reload silently; dirty tabs show the bar: Reload (discard draft), Keep mine (save
overwrites), Compare (side-by-side field diff using `similar`). Problem badge on the tree item +
a message naming file and line. Tests (use a tempdir workspace and the real watcher with a
bounded GPUI-timer wait): external_add_appears_in_tree, dirty_tab_shows_changed_on_disk_bar,
reload_discards_draft, keep_mine_overwrites_on_save, conflict_file_shows_badge_with_line.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel external` → `PASS n` (n ≥ 2); `scripts/pustman-v1-test.sh request_editor disk` → `PASS n` (n ≥ 3).

### S43 — Environment editor: shared, local and secret values; set-secret prompt on send
**Goal:** the user can edit environments, collection variables and globals:
- Shared values are saved to files and local values to storage.
- Secrets go to the keychain and never get a shared value.

Sending with an unset secret prompts the user to set it.
**Depends on:** S36, S38.
**Deploy delta:** none.
**Edits:**
- `crates/environment_picker/**`
- `crates/response_viewer/**`

```
Read docs/PRD.md §6.2 (REQ-VAR-SHARE-01..03), §8.6 UnresolvedSecret, the sharing AC. Environment
editor (item or modal): table of key, shared value, local value, kind (Default/Secret), enabled,
description; Secret rows have no shared-value field; local values in storage.local_values;
secret values via SecretStore on background tasks; saving writes the environment file via
workspace_store. UnresolvedSecret error panel (response_viewer) gets a "Set <name>" button that
opens the editor focused on that secret; after setting, re-send works. Tests:
shared_value_written_to_file_local_value_to_storage, secret_value_goes_to_keychain_not_file,
secret_row_has_no_shared_value_field, unresolved_secret_panel_offers_set_and_resend.
```

**Verify:**
- `scripts/pustman-v1-test.sh environment_picker` → `PASS n` (n ≥ 7).
- `scripts/pustman-v1-test.sh response_viewer secret` → `PASS n` (n ≥ 1).

### S44 — `git` crate + status bar branch and dirty count
**Goal:** inside a Git repository, the status bar shows the branch and the number of dirty files.
Both refresh on watcher events and on focus.
**Depends on:** S41. **Deploy delta:** none.
**Edits:**
- `crates/git/**`
- `crates/pustman/src/main.rs` (hotspot)
- `crates/pustman/Cargo.toml` (hotspot)

```
Read docs/PRD.md REQ-COLLAB-03, AGENTS.md (smol::process, never std::process::Command). Create
crates/git (domain: no gpui; smol): `status(root) -> Result<Option<GitStatus{branch, dirty}>>`
via `git -C <root> status --porcelain=v2 --branch` with smol::process; Ok(None) when not a repo
or git is not installed (no error toast — the item hides). A small GPUI status-bar item lives in
the git crate behind a `ui` feature OR in crates/workspace — choose the option that keeps the git
crate GPUI-free and record a Decision (if workspace, this step's Edits must not grow: put the item
in crates/git/src/status_item.rs under a `gpui` feature). Register init. Tests (tempdir repos made
with smol::process git init): status_reports_branch_and_dirty_count, non_repo_returns_none,
detached_head_reported.
```

**Verify:**
- `scripts/pustman-v1-test.sh git` → `PASS n` (n ≥ 3).
- `cargo tree -p git --no-default-features | grep -c gpui` → `0`.

### S45 — Import/export UI (Postman collection, environment, globals)
**Goal:** a user can import a Postman collection, environment or globals file into the open
workspace and sees a report. Imported secrets go to the keychain. Exporting a collection writes
Postman v2.1 with warnings for skipped items.
**Depends on:** S9, S42. **Deploy delta:** none.
**Edits:** `crates/collections_panel/**`.

```
Read docs/PRD.md §6.4 (REQ-IO-01/02/03/06), crates/api_model/src/postman.rs. import_export::Import
(file dialog; read on a background task with the 50 MB check before reading fully) → import →
save_item for each item → report modal (imported, preserved-but-not-executed scripts, unsupported
auth, warnings); environment import puts SecretValuesToStore into SecretStore; export via
context menu → file dialog → write; failures → toast. Tests:
import_collection_writes_files_and_shows_report, import_environment_secrets_go_to_keychain,
oversized_file_rejected_with_message, export_writes_v21_and_lists_skipped.
```

**Verify:** `scripts/pustman-v1-test.sh collections_panel import` → `PASS n` (n ≥ 3); `scripts/pustman-v1-test.sh collections_panel export` → `PASS n` (n ≥ 1).

### S46 — Quick open (`Ctrl/Cmd-P`)
**Goal:** a fuzzy finder over every request in the workspace that opens the chosen one.
**Depends on:** S41. **Deploy delta:** none.
**Edits:** `crates/command_palette/**`.

```
Read docs/PRD.md REQ-UI-05, crates/command_palette. Add a QuickOpen picker (same crate, reusing the
picker) listing requests from the open workspace snapshot as "Collection / Folder / Name" with
method badge; Enter opens/focuses the tab. Bound by quick_open::Toggle (already in keymaps). Tests:
quick_open_lists_all_requests_with_paths, fuzzy_matches_on_path_segments, enter_opens_request_tab.
```

**Verify:** `scripts/pustman-v1-test.sh command_palette quick_open` → `PASS 3`.

### S47 — Sharing AC: two clones of one Git repository
**Goal:** the PRD §6.3 sharing AC passes as an automated test using two clones of a local bare repository.
**Depends on:** S37, S43, S44. **Deploy delta:** none.
**Edits:** `crates/integration_tests/**`.

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
if `git` is not installed.
```

**Verify:** `scripts/pustman-v1-test.sh integration_tests sharing_ac_two_clones` → `PASS 1`.

### S48 — M2 SIGN-OFF (HUMAN)
**Goal:** the M2 exit criteria are proven by two people (or two clones) on real machines.
**Depends on:** S42, S43, S45, S46, S47. **Deploy delta:** none. **Edits:** none.

```
Human validation on Linux + macOS with two clones of one GitHub repo: A adds a request + secret
api_key, commits, pushes; B (Pustman open) pulls → request appears within 1 s, api_key unset,
send prompts to set it; `git show` has no secret. Also: import a real Postman v2.1 export; export
it back and re-import in Postman; branch switch with a conflict file shows the badge; status bar
branch/dirty. Record M2 GO/NO-GO; then /promote → release/pustman-v1-4. Then decide which of
STUB-M3..M6 to expand first.
```

**Verify:** the log records `M2 GO`. CI is green at the merge SHA.

---

# Phase 6+ — Later milestones (STUBS: not dispatchable)

Each stub is expanded into full `S<N>` steps by a human-approved planning session after the
previous milestone's sign-off. The expansion follows the same shape and appends steps after S48. A
driver must **never** dispatch a STUB row; reaching one is a stop condition.

| Stub | Milestone | Crates | Depends on | Verify source (PRD) |
|---|---|---|---|---|
| STUB-M3 | GraphQL + SSE + full auth (OAuth2 PKCE, Digest, SigV4, JWT, client certs, CA, proxy) | http_engine, request_editor, response_viewer, secrets | S48 | §5.2, §5.3, REQ-HTTP-06/07/10/11; M3 exit: OAuth2 PKCE against a local test IdP, 10k-event SSE smooth |
| STUB-M4 | WebSocket + Socket.IO + Message log | websocket_client ★, response_viewer (Message log), console | S48 | §5.4, §5.5, §8.4; echo + Socket.IO server ACs; 100k-event log smooth |
| STUB-M5 | gRPC | grpc_client ★, request_editor | S48 | §5.6; reflection + proto import, all four call kinds |
| STUB-M6 | MQTT | mqtt_client ★ | S48 | §5.7; pub/sub round trip v3.1.1 + v5 on a local broker |
| STUB-M7 | v1 hardening (Git panel, GraphQL introspection, OpenAPI import, HAR export + its redaction assertion, OAuth1, NTLM) | git, collections_panel, console, api_model | STUB-M3, STUB-M4, STUB-M5, STUB-M6 | §11 M7; v2.1 import corpus ≥ 99%; accessibility checklist |

STUB-M4, STUB-M5 and STUB-M6 can be expanded and run in parallel (PRD §11). Each needs its own hotspot
review, because each adds a `Request` enum variant in `crates/api_model/src/request.rs`.

## Notes on "standalone & verifiable"

- **Additive only.** Every step adds a crate, a module or a UI surface, and no earlier behavior is removed.
  There is no flag, because nothing reaches users until a milestone tag is built and smoked.
- **Rollback** is reverting the step's squash commit, or rebuilding the previous `release/pustman-v1-<n>` tag.
- A red Verify stops the step there. Never stack the next step on red.
- **Domain steps (S1–S10)** verify on this machine today. **`[GPUI]` steps** verify after G-DEPS, and in
  CI on GitHub (which installs the libraries) from S1 on.
