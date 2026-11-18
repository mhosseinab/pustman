# Pustman v1 (M0–M2) — Progress

**Session-start ritual:** `pwd` → `scripts/pustman-v1-init.sh` (exists after S0) → `git log --oneline -15` →
read this file → `tail -n 30 docs/progress/pustman-v1/log.md` → `scripts/pustman-v1-verify.sh | tail -1`
prints `VERIFY OK` (after S0).

| | |
|---|---|
| Source | [`pustman-v1-implementation-steps.md`](pustman-v1-implementation-steps.md) (read-only) · plan [`pustman-v1-plan.md`](pustman-v1-plan.md) · driver [`pustman-v1-orchestrator-prompt.md`](pustman-v1-orchestrator-prompt.md) |
| Integration branch | `pustman-v1/main` (created by S0's merge; until then `main`) |
| Worker branches | `pustman-v1/S<N>` off `pustman-v1/main`, own worktree at **`.claude/worktrees/pustman-v1/S<N>`** (build cache = the worktree own `target/`; `.claude/worktrees/pustman-v1/.cargo/config.toml` sets `jobs = 4`) |
| Initial commit | `8b2f0df` (docs seeded; before-S0 commit) |
| Records / log | `docs/progress/pustman-v1/steps/S<N>.md` · `docs/progress/pustman-v1/log.md` |
| Fast verify | `scripts/pustman-v1-test.sh <crate> [filter]` → `PASS <n>` |
| Full verify | `scripts/pustman-v1-verify.sh` → `VERIFY OK` |
| Tripwire | `scripts/pustman-v1-tripwire.sh` → `TRIPWIRE 0` (baseline 0) |
| Review routing | **always-on:** `test-integrity-reviewer`, `spec-conformance-reviewer` · **GPUI UI crates:** `gpui-ui-reviewer` + `ux-auditor` (repo-native) · **api_model/variables/http_engine:** `http-engine-engineer` (review mode) · **secrets/redaction/TLS/import/history:** `security-reviewer` · **storage/workspace_store:** `state-integrity-reviewer` (+ `migration-reviewer` on migrations) · **hotspots S12/S20/S39/S30:** + `performance-reviewer` · **CI/hooks/scripts:** `infra-ops-reviewer` · **api_model public types (S2/S9):** + `contract-drift-reviewer` · **implementers:** GPUI→`ui-implementer`, http_engine/variables/api_model→`http-engine-engineer`, others→`step-implementer`; tests delegated to `gpui-test-writer` · **S0 reviewer:** `infra-ops-reviewer` (skill agents land with S0) · missing agent → fresh general-purpose subagent with matching contract |

**Writers:** only the orchestrator and the integrator write this file, the log and step records. Workers,
fixers and reviewers never do.

## Gates and rules in force

- **G-DEPS** (after S10), in order: install the GPUI system libraries
  (`libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev`) until
  `scripts/pustman-v1-init.sh --require-gpui` exits 0; push for CI (or waive it in writing); then
  promote `release/pustman-v1-1` (no app smoke). This blocks S11, S17, S21 and everything after them.
- **Human sign-offs:** S25 (M0), S38 (M1), S48 (M2). Each one needs `M<k> GO` from the human.
- **STUB-M3..M7:** never dispatchable. They are expanded in a separate planning session.
- **Budgets:** 4 review cycles per step, plus at most 2 on measurable progress. The turn budget comes from the segment's `/goal` line.
- **Window cap:** unapplied deploy deltas ≥ 3 or unpromoted steps ≥ 16. "Applied" follows plan D11:
  - S0 hooks and S6 migrations are marked applied at merge, with evidence from the full verify.
  - S1 CI is applied when the human has pushed and CI is green.
  - S0's manual delta is applied at G-DEPS.
- **Hotspots:** `crates/pustman/src/main.rs`, `crates/pustman/Cargo.toml` and `Cargo.lock`. Hotspot steps are dispatched and merged one at a time, and `Cargo.lock` is regenerated at merge.
- **High-stakes steps** need 2 independent reviewers:
  - secrets/redaction: S3, S7, S16, S37, S43, S47
  - persistence: S5, S6, S8
  - TLS: S12, S28
  - import: S9, S45
- **Severity:** MEDIUM and HIGH findings stop the step. LOW findings become `gh issue create --label "pustman-v1,low"`.
- **UI steps** (crates `ui`, `theme`, `workspace`, `text_input`, any `*_panel`/`*_editor`/`*_viewer`/`*_picker`, `command_palette`, `assets/themes`, `assets/icons`): every worker, reviewer and fixer prompt MUST include the three lines in Active CARRY-FORWARD (load the `rust-gpui-ui-ux` skill + its "Design source" section; read the redesign pack from `/home/geek/workspace/rea-agent/pustman/docs/postman_redesign_pack/` read-only, never copy it or `03_references/screenshots/`/`07_guides/` into the repo; cite screen IDs S01–S24 and requirement IDs R01–R28 in `Decision:` lines — S16–S23 cloud collaboration is out of v1 per `docs/PRD.md`). Send UI steps to `ux-auditor` alongside the usual reviewers. If a step's Verify contradicts the pack, STOP and ask the human.

## Deferred verification gate

These run at the next human sign-off and are recorded in that sign-off's step record.

| Item | Owner | Due at | Status |
|---|---|---|---|
| E2E display scenario: S22 (shell, docks, toasts) | human | S25 | pending |
| Live keyring: `cargo test -p secrets -- --ignored` | human | S25 | pending |
| E2E display scenario: S26 (send/cancel from the URL bar) | human | S38 | pending |
| E2E display scenario: S28 (TLS badge, auth tabs) | human | S38 | pending |
| E2E display scenario: S32 (console dock) | human | S38 | pending |
| Push `pustman-v1/main` and confirm CI green (S1 delta) | human | next promotion | done 2026-11-18 (CI run 44240566190 on `e5347fc`: verify (ubuntu-latest) success, verify (macos-latest) success) |
| Manual smoke on Linux + macOS for each `release/pustman-v1-<n>` (n ≥ 2) | human | each promotion | pending |

## Window

Last tag: _none_ · unpromoted passed steps: 5 (S0, S1, S2, S6, S7) · unapplied deltas: 0 (cap 3 / 16).

## Status

`pending | in_progress | passed | blocked`. Att. = worker dispatches, including fixers.

| Step | Title | Status | Att. | SHA | Record | Note |
|---|---|---|---|---|---|---|
| S0 | Harness: bootstrap, test runner, tripwire, hooks, promotion tooling, subagents | passed | 1 | `4cc3089` | [S0](steps/S0.md) | review PASS (no findings); full verify `VERIFY OK (no workspace yet)` at merge |
| S1 | Cargo workspace, toolchain, lints, CI, `api_model` skeleton | passed | 1 | `1078d67` | [S1](steps/S1.md) | PASS_WITH_ISSUES (1 LOW → issue #1); verify `VERIFY OK`; CI delta pending push+green |
| S2 | `api_model` types: collections, folders, HTTP requests, auth, variables, settings, examples | passed | 3 | `1f77523` | [S2](steps/S2.md) | 2 cycles (1 HIGH fixed); verify `PASS 15`; LOWs → issues #2/#3/#4 |
| S3 | Shared `redact()` (D8) | in_progress | 1 | `f6a6ade` | | worker done (PASS 5); review pending (HIGH-STAKES, 2 reviewers) |
| S4 | `variables` resolver | in_progress | 2 | `ca4cddd` | | worker done (PASS 8); review pending |
| S5 | `workspace_store`: on-disk format, deterministic atomic writes | passed | 2 | `b08d9a8` | [S5](steps/S5.md) | 3 review rounds, 3 fix cycles; R3 PASS / PASS_WITH_ISSUES (1 pre-existing api_model LOW → issue #7); `PASS 30` |
| S6 | `storage`: SQLite local store with migrations | passed | 3 | `a84c5a3` | [S6](steps/S6.md) | 3 review rounds, 2 fix cycles (2 data-loss MEDIUMs + 2 proof-quality MEDIUMs closed); `PASS 22`; LOWs → issues #5/#6 |
| S7 | `secrets`: OS keychain behind a trait | passed | 1 | `626aa12` | [S7](steps/S7.md) | 2 reviewers converged on 1 MEDIUM → fixed; re-review PASS; live keyring → S25 gate |
| S8 | `workspace_store` watcher: external changes and problems | pending | 0 | | |  |
| S9 | Postman v2.1/v2.0 import, v2.1 export, environment/globals import | pending | 0 | | |  |
| S10 | cURL import and export | pending | 0 | | |  |
| G-DEPS | Install GPUI system libraries, CI push, then promote `release/pustman-v1-1` | pending | – | | | HUMAN gate; 1/3 libraries installed, 2/3 CI green (run 44240566190); 3/3 promote open |
| S11 | `http_engine` core: transport trait, fake, error taxonomy, send/cancel | pending | 0 | | | after G-DEPS |
| S12 | `ReqwestTransport`: settings, manual redirects, TLS toggle, timing spike (D2) | pending | 0 | | |  |
| S13 | Send pipeline: variable resolution, auth (Basic/Bearer/API key), unresolved secrets | pending | 0 | | |  |
| S14 | Request bodies, file refs, auto headers, large response spill | pending | 0 | | |  |
| S15 | Cookie jar per workspace | pending | 0 | | |  |
| S16 | History recording with redaction, 1 MB cap, retention | pending | 0 | | |  |
| S17 | `settings` crate | pending | 0 | | | after G-DEPS |
| S18 | `theme` crate with bundled light and dark themes | pending | 0 | | |  |
| S19 | `ui` design-system crate: tokens and base components | pending | 0 | | |  |
| S20 | `text_input` crate: single- and multi-line editing | pending | 0 | | |  |
| S21 | `pustman_actions` + complete default keymaps (D10) | pending | 0 | | | after G-DEPS |
| S22 | `pustman` binary + `workspace` shell (window, docks, panes/tabs, status bar, toasts) | pending | 0 | | |  |
| S23 | Tab and layout restore | pending | 0 | | |  |
| S24 | Command palette | pending | 0 | | |  |
| S25 | M0 SIGN-OFF (HUMAN) | pending | 0 | | | HUMAN sign-off |
| S26 | `request_editor`: URL bar, method, send/cancel, response status line | pending | 0 | | |  |
| S27 | Params, path variables, headers tables; cURL paste/copy | pending | 0 | | |  |
| S28 | Body, auth and settings tabs (TLS badge) | pending | 0 | | |  |
| S29 | `response_viewer`: Pretty/Raw body, headers, error panel | pending | 0 | | |  |
| S30 | Response: Hex, Preview, Find, save/copy, large bodies, Cookies tab | pending | 0 | | |  |
| S31 | Timeline tab: phases, network info, redirect chain, resolved request as sent | pending | 0 | | |  |
| S32 | `console` crate (bottom dock) | pending | 0 | | |  |
| S33 | Variable highlighting, autocomplete, hover, pre-send warning | pending | 0 | | |  |
| S34 | `environment_picker` + quick-look | pending | 0 | | |  |
| S35 | `history_panel` | pending | 0 | | |  |
| S36 | Manage Cookies dialog | pending | 0 | | |  |
| S37 | Redaction NFR test (integration) | pending | 0 | | |  |
| S38 | M1 SIGN-OFF (HUMAN) | pending | 0 | | | HUMAN sign-off |
| S39 | `collections_panel`: tree | pending | 0 | | |  |
| S40 | Collection CRUD, save, ordering, dirty state | pending | 0 | | |  |
| S41 | Open, create and recent workspaces; format-version banner | pending | 0 | | |  |
| S42 | External change bar and problem badges | pending | 0 | | |  |
| S43 | Environment editor: shared, local and secret values; set-secret prompt on send | pending | 0 | | |  |
| S44 | `git` crate + status bar branch and dirty count | pending | 0 | | |  |
| S45 | Import/export UI (Postman collection, environment, globals) | pending | 0 | | |  |
| S46 | Quick open (`Ctrl/Cmd-P`) | pending | 0 | | |  |
| S47 | Sharing AC: two clones of one Git repository | pending | 0 | | |  |
| S48 | M2 SIGN-OFF (HUMAN) | pending | 0 | | | HUMAN sign-off |
| STUB-M3 | GraphQL + SSE + full auth (OAuth2 PKCE, Digest, SigV4, JWT, client certs, CA, proxy) | not dispatchable | – | | | expand in a planning session |
| STUB-M4 | WebSocket + Socket.IO + Message log | not dispatchable | – | | | expand in a planning session |
| STUB-M5 | gRPC | not dispatchable | – | | | expand in a planning session |
| STUB-M6 | MQTT | not dispatchable | – | | | expand in a planning session |
| STUB-M7 | v1 hardening (Git panel, GraphQL introspection, OpenAPI import, HAR export + its redaction assertion, OAuth1, NTLM) | not dispatchable | – | | | expand in a planning session |

## Open work

_None._

## Active CARRY-FORWARD

Each entry is a fact a later step must know, with its source record linked.

- **UI design source is `docs/postman_redesign_pack/` (2026-11-17, operator).** UI steps now use it as the design source, per the `rust-gpui-ui-ux` skill's "Design source" section. The pack is NOT in git — read it from `/home/geek/workspace/rea-agent/pustman/docs/postman_redesign_pack/`, treat as read-only, never copy it (or `03_references/screenshots/`, `07_guides/`) into the repo. Cite screen IDs S01–S24 and requirement IDs R01–R28 in `Decision:` lines; screens S16–S23 (cloud collaboration) are out of v1 scope per `docs/PRD.md`. Precedence: PRD decides scope, the pack decides in-scope design, project rules always apply. If a step's Verify contradicts the pack, stop and ask the human. Source: merge `chore: merge main (redesign-pack UI guidance, trimmed evidence log)` (brings `d965f21`).

## Learnings (≤ 15)

- **Dispatch (OPERATOR decisions 2026-11-18, two of them).** Paths: worktrees at `.claude/worktrees/pustman-v1/S<N>`, briefs at `.claude/worktrees/pustman-v1/.briefs/S<N>-a<k>.md` (`-review<c>`, `-fix<c>`), build cache = the worktree own `target/` (no `CARGO_TARGET_DIR`), `CARGO_BUILD_JOBS=4` kept and also set by `.claude/worktrees/pustman-v1/.cargo/config.toml`. Briefs + dispatch: a brief file carries the real content and a `BRIEF-TOKEN`; the dispatch prompt is < ~400 chars and the worker must echo the `BRIEF-TOKEN` back (proof it read the brief). Isolation: one worker = one worktree = one branch; a worker never touches another worktree, the main checkout, or `pustman-v1/main`. Hotspot steps (`crates/pustman/src/main.rs`, `crates/pustman/Cargo.toml`) run alone. Merges one at a time. Reviewers stay in pairs and may run alongside workers. **Load guard before every dispatch:** `free -g` available >= 6 GB AND `df -h /home/geek/workspace/rea-agent/pustman` >= 40 GB free, else hold and log `LOAD HOLD`. Empty replies: check `git log pustman-v1/main..pustman-v1/S<N>` first; re-send once on its own while the others continue; 2 or more empty-with-no-commit in one batch -> drop to 2 for the next batch and log, return to the cap after one clean batch; log each empty with prompt length, brief path, elapsed time and the `free -g` reading. On merge: `git worktree remove .claude/worktrees/pustman-v1/S<N>` (deletes its `target/`), keep the branch. **CAP = 3** (OPERATOR confirmed 2026-11-18). Nothing goes out at width > 3. (2026-11-18)
- **Worktree move invalidates `env!()`-baked paths.** A worktree moved from `/tmp` into `.claude/worktrees/pustman-v1/` keeps its `target/`, but `env!("CARGO_MANIFEST_DIR")` was expanded at COMPILE time against the old path, so cargo reuses a test binary whose baked path no longer exists and `read_dir` returns NotFound. **After any `git worktree move`, `rm -rf <worktree>/target` and rebuild.** (2026-11-18)

## Promotions

| Tag | SHA | Steps | promote-check | Build | Smoke (Linux / macOS) | Brief |
|---|---|---|---|---|---|---|
