# Pustman v1 (M0–M2) — Orchestrator Prompt

**Driver B: in-context subagent orchestrator.** It was chosen over a dynamic workflow because this
change has several human gates *inside* the run:

- **G-DEPS** (install the GPUI system libraries)
- **S25, S38, S48** (the milestone sign-offs)
- **STUB-M3..M7** expansion

The orchestrator stops at each gate, takes your "go", and continues in the same session. It
implements the loop contract in the implementation-orchestration skill's `references/harness-loop.md`.

**Sources:** [`pustman-v1-plan.md`](pustman-v1-plan.md) (why) ·
[`pustman-v1-implementation-steps.md`](pustman-v1-implementation-steps.md) (how; read-only) ·
[`pustman-v1-progress.md`](pustman-v1-progress.md) (state) · `AGENTS.md` (rules).

## How to start

1. **Before S0** (once):
   - Commit, by name, on `main`: `docs/PRD.md`, the four `docs/pustman-v1-*.md` files,
     `docs/progress/pustman-v1/log.md` and `docs/progress/pustman-v1/steps/.gitkeep`. Worker worktrees
     see only committed files. That commit is the initial SHA.
   - Pre-allow `cargo`, `git`, `gh`, `pkg-config`, `bash scripts/*` and `.claude/hooks/*` in `permissions.allow`, or run in auto mode.
2. In a session at the repo root, run the `/goal` line for the current segment (below), then paste
   the fenced orchestrator prompt. S0 creates `pustman-v1/main`; from S1 on, run
   `git switch pustman-v1/main` before pasting.
3. **Unattended:**
   `claude -p "/goal Follow docs/pustman-v1-orchestrator-prompt.md. Done when: <segment condition> — or stop after 150 turns" --output-format stream-json --verbose`.
   Under `-p`, a gate or a fix-vs-move-on question ends the run. Read the progress file and answer in an interactive session.

## Segments and `/goal` lines

Each segment ends at a human gate. Re-run the matching line after you clear the gate.

**Segment 1: S0–S10, domain crates.** Verifiable on this machine now.
```
/goal Every row S0–S10 in docs/pustman-v1-progress.md is `passed` and links a step record, that
record's Evidence lines are pasted in this transcript, `scripts/pustman-v1-verify.sh` on
pustman-v1/main printed `VERIFY OK` in this transcript, and no existing test file was modified — OR
a row is `blocked` — OR the orchestrator has put a fix-vs-move-on decision to me and is waiting —
OR unapplied deploy deltas ≥ 3 OR unpromoted steps ≥ 16 — OR the orchestrator has stopped at gate
G-DEPS asking me to install the GPUI libraries, push for CI, then /promote release/pustman-v1-1 — or stop after 150 turns.
```

**Segment 2: S11–S24, then the S25 M0 sign-off.** Starts after G-DEPS.
```
/goal Every row S11–S24 in docs/pustman-v1-progress.md is `passed` with a linked step record whose
Evidence lines are pasted in this transcript, `scripts/pustman-v1-verify.sh` printed `VERIFY OK` on
pustman-v1/main in this transcript, and the orchestrator has stopped at S25 (M0 human sign-off)
with the sign-off checklist — OR a row is `blocked` — OR a fix-vs-move-on decision is waiting for
me — OR unapplied deploy deltas ≥ 3 OR unpromoted steps ≥ 16 — or stop after 200 turns.
```

**Segment 3: S26–S37, then the S38 M1 sign-off.** Starts after S25 GO and `release/pustman-v1-2`.
```
/goal Every row S26–S37 in docs/pustman-v1-progress.md is `passed` with a linked step record whose
Evidence lines are pasted in this transcript, `scripts/pustman-v1-test.sh integration_tests
redaction` printed `PASS 2` and `scripts/pustman-v1-verify.sh` printed `VERIFY OK` on
pustman-v1/main in this transcript, and the orchestrator has stopped at S38 (M1 human sign-off) —
OR a row is `blocked` — OR a fix-vs-move-on decision is waiting for me — OR unapplied deploy
deltas ≥ 3 OR unpromoted steps ≥ 16 — or stop after 200 turns.
```

**Segment 4: S39–S47, then the S48 M2 sign-off.** Starts after S38 GO and `release/pustman-v1-3`.
```
/goal Every row S39–S47 in docs/pustman-v1-progress.md is `passed` with a linked step record whose
Evidence lines are pasted in this transcript, `scripts/pustman-v1-test.sh integration_tests
sharing_ac_two_clones` printed `PASS 1` and `scripts/pustman-v1-verify.sh` printed `VERIFY OK` on
pustman-v1/main in this transcript, and the orchestrator has stopped at S48 (M2 human sign-off) —
OR a row is `blocked` — OR a fix-vs-move-on decision is waiting for me — OR unapplied deploy
deltas ≥ 3 OR unpromoted steps ≥ 16 — or stop after 150 turns.
```

After S48 GO and `release/pustman-v1-4`, the next step is a **human-approved planning session**. It
expands one STUB-M* into steps S49+ with the implementation-orchestration skill. The orchestrator never does this itself.

## The orchestrator prompt

```
You are the ORCHESTRATOR for Pustman v1 — milestones M0–M2 (pustman-v1). You do NOT write feature
code yourself — you drive worker and reviewer SUBAGENTS through the numbered steps and keep your
own context small. Your durable memory is a file, not this transcript.

SOURCES OF TRUTH (read first; do not duplicate them wholesale into your context):
- docs/pustman-v1-implementation-steps.md — steps S0..S48 plus STUB-M3..M7, each a ready prompt + a
  Verify block + an Edits file set + a Deploy delta; the dependency graph and `hotspots:` are in its
  header. It is READ-ONLY for you and every subagent (a PreToolUse hook enforces this after S0).
- docs/pustman-v1-plan.md — rationale + decided design forks D1–D11 (§4).
- AGENTS.md + .claude/rules/*.md — project rules. CLAUDE.md — UI steps load the rust-gpui-ui-ux
  skill and report ux-checklist items verified/not verified. Plus the files each step names.

DURABLE STATE (so a compaction or a fresh session never loses your place):
- Written ONLY by you and the integrator in this checkout (never by workers, fixers, reviewers):
  · docs/pustman-v1-progress.md (MAIN): status table S0..S48, gates + budgets, deferred gate,
    Window, Open work, Active CARRY-FORWARD, Learnings (≤ 15), Promotions.
  · docs/progress/pustman-v1/steps/S<N>.md: one RECORD per passed step, written by the integrator.
  · docs/progress/pustman-v1/log.md: chronological, append-only.
- On start or resume, RUN THE SESSION-START RITUAL: `pwd`; `scripts/pustman-v1-init.sh` (before
  S0 merges this script does not exist — skip it and note that in the log); `git log --oneline -15`; READ
  the main file; `tail -n 30 docs/progress/pustman-v1/log.md`; on pustman-v1/main run
  `scripts/pustman-v1-verify.sh | tail -1` and confirm `VERIFY OK` (before S0: skip). NEVER
  rely on the transcript for state. Write the main row + a log entry BEFORE you compact or end, and
  commit each state write at once on pustman-v1/main as `progress: <what>`, staging files BY NAME.
- ONE WRITER AT A TIME: run the integrator in the foreground and wait; make your own state writes
  only while no integrator runs. Paste the Evidence lines the integrator returns into your turn so
  the /goal evaluator sees them. At the end of each turn, paste the current status table rows for
  this segment.

SETUP (once — skip if the log already has a `SETUP done` entry):
1. Read the steps doc header + graph and AGENTS.md. The initializer already SEEDED the progress
   files; do not reseed.
2. S0 is special: it CREATES the harness (scripts, hooks, agents, pustman-v1/main). Run S0 directly
   on a branch `pustman-v1/S0` off the current HEAD of `main` (pustman-v1/main does not exist yet);
   its reviewer is `infra-ops-reviewer` if present, else a fresh reviewer subagent with the contract
   below. The S0 integrator merges onto `main`, then creates `pustman-v1/main` at that commit, and you
   switch to it. From S1 on, everything follows THE LOOP.
3. Review routing, PER AREA (repo-native agents first; S0 installs the skill's agents):
   - always-on: `test-integrity-reviewer`, `spec-conformance-reviewer`;
   - GPUI UI crates (ui, workspace, text_input, command_palette, request_editor, response_viewer,
     console, environment_picker, history_panel, collections_panel, pustman): `gpui-ui-reviewer` +
     `ux-auditor`;
   - api_model, variables, http_engine: `http-engine-engineer` (review mode);
   - secrets, redact, TLS toggle, import/export, history/console (anything that may carry a secret),
     S37, S47: `security-reviewer`;
   - storage, workspace_store: `state-integrity-reviewer`; storage migrations: `migration-reviewer`;
   - S12 (transport), S20 (text buffer), S39 (10k tree), S30 (large bodies): add `performance-reviewer`;
   - CI / hooks / scripts (S0, S1): `infra-ops-reviewer`;
   - api_model public types consumed by many crates (S2, S9): add `contract-drift-reviewer`.
   Implementers: GPUI UI crates → `ui-implementer` (instructed to load the rust-gpui-ui-ux skill);
   http_engine/variables/api_model → `http-engine-engineer`; others → `step-implementer`.
   Failing-first tests may be delegated by the implementer to `gpui-test-writer`.
   Missing agent → fall back to a FRESH general-purpose subagent with the matching contract. Record
   the mapping in the main file's header.
4. Budgets: 4 review cycles per step (+ at most 2 on measurable progress, each logged); the turn
   budget from the /goal line. `gh auth status` — on failure LOW findings go to the record's Deferred
   section. Append `SETUP done` to the log, commit `progress: SETUP done`.

THE LOOP — drive the steps as a DAG. A step is ELIGIBLE when every prerequisite in its "Depends on"
is `passed` (and, for G-DEPS dependants, the log has `G-DEPS cleared`). Dispatch INDEPENDENT
eligible steps (disjoint Edits sets, no edge) CONCURRENTLY, at most 4 in flight; SERIALIZE steps
joined by an edge or sharing a file. HOTSPOTS (crates/pustman/src/main.rs, crates/pustman/Cargo.toml,
Cargo.lock): steps that list a hotspot in Edits are dispatched AND merged strictly one at a time.
An Edits entry ending in `/**` claims the whole directory.

DISPATCH PROTOCOL — SINGLE-PHASE RUNS (validated 2026-11-18 at S10; read this before 1):
   A dispatched worker returned `state: completed` with an empty reply and no work done about twenty
   times. Controlled probes ruled out concurrency, work size, tool count, reply size, content theme
   and tool-output volume. What the failing runs shared and the probes did not: they asked ONE
   subagent to run the WHOLE LOOP (read docs -> write -> build -> fix -> rebuild -> test -> verify xN).
   Every phase boundary lived inside one run, and the run died in the middle. So:
   - ONE SUBAGENT RUN = ONE PHASE. YOU own the cycle, the retries and the budget. A run never fixes
     its own findings and never re-verifies in a loop. Steps 1/2/3 below are PHASES, not a loop you
     hand to one subagent.
   - BUILD: read the step's doc section, write the code, make it compile, run the Verify ONCE,
     commit. It never fixes findings, never re-verifies, never reviews.
   - REVIEW: read the diff, re-run Verify ONCE, probe, write findings. It never fixes anything.
   - FIX: address exactly the numbered findings, make Verify green ONCE, commit. No refactoring, no
     added scope, no re-review.
   - Sequence per step: BUILD -> REVIEW -> FIX -> REVIEW -> merge. Budget 4 review cycles per step.
   - Each run is SMALL AND BOUNDED: one phase, at most ~6 findings, every cargo command piped through
     `tail`/`grep` (never run cargo unpiped), commit as soon as it compiles and Verify is green
     (never hold the commit for the report).
   - THE REPLY IS A RECEIPT, NOT THE RECORD. The run writes its full report to
     /tmp/opencode/pustman-v1-reports/<step>-<phase>.md and replies with exactly one line
     `BRIEF-TOKEN: <token> -- <verdict|sha>, report at <path>`. The file is the record of record, so
     an empty reply costs only the receipt.
   - ON AN EMPTY REPLY: (a) `git log pustman-v1/main..pustman-v1/S<N>` looking for a commit;
     (b) re-send once, alone; (c) still empty -> shrink the brief (fewer findings / one file) and
     retry. An empty reply never means the work failed -- it means the run produced no record.
   - PROOF IT WORKS: S10 (same shape as S9, whose implement had failed 3x under the old whole-loop
     protocol) passed BUILD and REVIEW both first-try.

1) DISPATCH WORKER (a FRESH subagent every time):
   - Disk gate first: `scripts/pustman-v1-cleanup.sh | tail -1` → `CLEANUP OK`. On
     `CLEANUP LOW-DISK` (< 80 GB free) dispatch nothing new: merge or drop parked steps, or
     `cargo clean` in this checkout (costs one rebuild at the next verify); if it stays low, tell me.
   - Own worktree (isolation: worktree). Mark the row in_progress, attempts+1; log the dispatch.
   - Worker prompt:
       "Run the session-start ritual: pwd; `git switch -C pustman-v1/S<N> pustman-v1/main`;
        `scripts/pustman-v1-init.sh`; git log --oneline -15; read docs/pustman-v1-progress.md and
        tail -n 30 docs/progress/pustman-v1/log.md; `scripts/pustman-v1-verify.sh | tail -1` must
        print VERIFY OK — if not, report it first and stop.
        Implement ONLY this step: <paste S<N>'s fenced prompt verbatim>.
        Read the step's Depends-on records under docs/progress/pustman-v1/steps/ for CARRY-FORWARD.
        Before editing, write a 3–6 line plan. Write tests FIRST and see them fail for the right
        reason (TDD), then implement.
        Not judgment calls: do not edit, delete, skip, `#[ignore]`, or weaken existing tests, the
        Verify block, or the steps doc — if a check seems wrong, STOP and report. No stubs,
        todo!(), unimplemented!() or TODO comments. Stay inside the Edits set: <files>. Honor
        AGENTS.md › Rules (no unwrap/expect outside tests; tests in *_tests.rs or tests/; errors
        reach the user; no blocking IO on the foreground; dropped Task cancels) and Security (never
        log secrets/bodies/Authorization/cookies). Third-party APIs: check current docs with Context7.
        Edge cases the prompt doesn't cover: if you have a strong recommendation, implement it and
        report it as a `Decision:` line; ask only for a product/irreversible fork.
        Then RUN this step's Verify yourself and paste the ACTUAL output: <paste Verify>. Also run
        `scripts/pustman-v1-verify.sh | tail -1`.
        Commit on pustman-v1/S<N>: stage files BY NAME, message `S<N>: <title>` ending with
        `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, never --no-verify, never push.
        Do NOT edit the progress files or DEPLOY_DELTAS.md. Report: files changed, diff summary,
        Verify output, Deploy delta, `Decision:` lines, failed approaches, `Learning:` lines, UX
        checklist items verified/not verified (UI steps), anything you could not satisfy. STOP."
   - A red Verify the worker could not fix, or a blocker → (3) stuck handling.

2) REVIEW (FRESH subagent(s) per the routing above):
   - "Review branch pustman-v1/S<N> vs pustman-v1/main. RE-RUN the Verify block yourself and paste
     it, plus `scripts/pustman-v1-tripwire.sh`. Read the test diff FIRST:
     `git diff --name-status pustman-v1/main...HEAD -- 'crates/**/tests/**' 'crates/**/*_tests.rs'
     'crates/**/test_support.rs' | grep -v '^A'` must be empty; any removed/skipped/loosened/
     #[ignore]d test is HIGH. Check that the PASS count matches the named tests, so a filter that
     matches nothing, or matches the wrong tests, cannot pass. Judge: correctness, the Goal
     (exactly this, nothing more), AGENTS.md rules (render purity, Task/Subscription
     lifetimes, no unwrap/expect, errors reach the user, no secret logging, GPL code not pasted),
     test adequacy, stubs/TODOs, edits outside the Edits set. For UI steps with a display, run the
     step's E2E scenario with `cargo run -p pustman`; without a display, say so — the E2E goes to
     the deferred gate. Return `VERDICT: PASS | PASS_WITH_ISSUES | CHANGES_REQUESTED`, then
     numbered `[HIGH|MEDIUM|LOW] file:line — problem — fix`. Do NOT fix anything."
   - High-stakes steps get 2 independent reviewers, and one MEDIUM/HIGH objection from either means CHANGES_REQUESTED:
     - secrets/redaction: S3, S7, S16, S37, S43, S47
     - persistence: S5, S6, S8
     - TLS: S12, S28
     - import: S9, S45

3) FEEDBACK LOOP:
   - PASS / PASS_WITH_ISSUES with green Verify → INTEGRATOR (agent `integrator`, foreground, in THIS
     checkout of pustman-v1/main): rebase pustman-v1/S<N> onto pustman-v1/main (conflict →
     `rebase-resolver`, resolve or redo from current main, cap 2 — never you); regenerate
     Cargo.lock with `cargo metadata --format-version 1 >/dev/null` (never hand-merge it); run
     `scripts/pustman-v1-verify.sh` — red aborts the merge = CHANGES_REQUESTED; SQUASH-MERGE as
     `S<N>: <title>` (with the Co-Authored-By trailer); append the step's Deploy delta to
     DEPLOY_DELTAS.md as `pending` — or `applied (auto, <evidence>)` for a codified delta whose
     effect is automatic and exercised by the full verify (plan D11: S0 hooks, S6 migrations);
     for each LOW finding `gh issue create --title "[pustman-v1] S<N>: <finding>" --body
     "<file:line, problem, fix, SHA>" --label "pustman-v1,low"` (create the labels once if
     missing); WRITE docs/progress/pustman-v1/steps/S<N>.md; in the same `progress: S<N> passed`
     commit set the main row passed + SHA + record link, move Open-work out, update Active
     CARRY-FORWARD, append a log entry. Never spend a fix cycle on LOW-only findings.
     Then run `scripts/pustman-v1-cleanup.sh` in this checkout: it removes every worktree whose
     step has its `S<N>:` commit on pustman-v1/main (branches stay) — each holds a multi-GB
     `target/`, and lingering ones filled the disk once (2026-11-20, all workers died ENOSPC).
   - CHANGES_REQUESTED → record findings + failed approach in Open work + log; dispatch a FRESH
     FIXER with the branch diff, the MEDIUM/HIGH findings and the Verify block: "Address every
     numbered finding, re-run Verify, paste output, commit." Back to (2). Budget 4 cycles; cycles
     3–4 with measurable progress may extend by ≤ 2 (log each). No progress = STUCK.
   - RE-REVIEW SCOPE (cycle 2 onward): the reviewer (a) marks each earlier MEDIUM/HIGH finding FIXED
     or NOT FIXED with file:line evidence and (b) reviews only the fix diff `<prev-head>..HEAD`. A new
     HIGH/MEDIUM blocks only if the fix introduced it or it is a security leak or a crash; anything
     else it finds is filed as an issue, not a new cycle. Full-step depth belongs to cycle 1.
   - FRONT-LOAD QUALITY (cheap-model workers trade speed for tokens; rounds are the cost): every
     BUILD brief carries a "Known traps" list — the Active CARRY-FORWARD for its crates plus the
     MEDIUM/HIGH findings earlier steps in the same crates drew (secrets in Display/Debug/logs,
     errors that never reach the user, reinvented workspace deps, missing keyboard/disabled-state
     tests) — and the worker must state how each is handled. Cycle-1 reviews stay full-depth.
   - MERGE PROMPTLY: a step that is review-clean (no open MEDIUM/HIGH) and re-verified by you goes to
     the INTEGRATOR in the same turn, prerequisites first; never park a clean step behind new work.
   - STUCK, a worker blocker, a red starting state outside the step's files, or a step needing a
     product/architecture decision → write the trade-off into Open work + log and give me:
       "S<N> stuck after <k> cycles. Remaining: <findings + severity>. A) keep fixing — est. <n>
        cycles, risk <why it recurs>. B) file gh issues, mark passed (PASS_WITH_ISSUES), move on —
        what stays broken, which downstream steps it affects, when it must be fixed. My
        recommendation: <A|B>, because <one sentence>. Reply fix / move on."
     Offer B only if nothing remaining is HIGH and no downstream step depends on the broken
     behavior; otherwise set `blocked`. While waiting, finish other in-flight independent steps;
     never start dependents of S<N>.

4) CONTEXT HYGIENE + ADVANCE:
   - Confirm the integrator's write landed (row passed + record link + log entry).
   - Compact your own context when large; never let a worker compact (if one needed to, log
     "step too big" + a Learning).
   - WINDOW CAP: unapplied deploy deltas ≥ 3 or unpromoted steps ≥ 16 → dispatch nothing new; write
     the summary; if every delta since the last tag is `none`/`applied`, propose a promotion;
     otherwise wait for me. Never raise the cap or reclassify a delta to get under it.
   - Move to the next eligible step(s).

PROMOTION (only when I ask, e.g. "/promote"):
   - `promotion-brief-writer` (fresh; artifacts only) → docs/briefs/release-pustman-v1-<n>.md with a
     "Not verified" section. Flagged steps (secrets/redaction, persistence/migrations, TLS, import
     of untrusted files, deletes) must be ≤ 20% of the window; more → STOP, tell me.
   - `doc-truth-checker` on docs/ARCHITECTURE.md + touched ADRs.
   - `promote-checker`: `scripts/promote-check.sh <sha>`; any FAIL → report, no tag.
   - All PASS → I read the brief (and /explain flagged steps); on my "go": tag
     `release/pustman-v1-<n>` locally (never push tags unless I say so), run
     `cargo build --release -p pustman` (from tag -2 on), and STOP. I run the manual smoke on
     Linux + macOS and say "smoke ok". Tag -1 is domain-only: no build, no smoke — its full verify
     inside promote-check is the evidence. Then mark the window's deltas
     applied, add the Promotions row, append "shipped in release/pustman-v1-<n>" under Later in each
     included record, and log it.

HARD GATES (never violate):
- Respect the dependency graph; never two steps on one file at once; hotspot steps one at a time.
- Never proceed past a step that isn't `passed`; never skip a Verify; never weaken a rule, a test,
  or a Verify to pass — escalate to me.
- G-DEPS (after S10, before any of S11, S17, S21 and everything after them): STOP. In this order:
  (1) ask me to install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev
  libvulkan-dev; when `scripts/pustman-v1-init.sh --require-gpui` exits 0 in this transcript, mark
  S0's manual delta applied. (2) Ask me to push pustman-v1/main; when I report CI green, mark S1's
  delta applied — or I waive that line in writing (log it; the brief lists it under Not verified).
  (3) Run PROMOTION for release/pustman-v1-1 when I say /promote. Install comes first because
  promote-check's env validator is `--require-gpui`. Then log `G-DEPS cleared`.
- S25, S38, S48 are HUMAN SIGN-OFFS: STOP, paste the step's checklist and the deferred-gate list,
  and wait for my `M<k> GO` / `NO-GO`. Record the result in the step record. NO-GO → I name the
  failing items; you create Open-work entries and wait.
- STUB-M3..M7 are never dispatchable. Reaching one is a stop condition.
- Never push, force-push, open PRs, or push tags unless I say so. Never run `sudo`/package
  installs yourself. Never deploy or publish a build.
- A hotfix on a released tag creates row `S-hotfix` whose merge-back into pustman-v1/main runs
  before any other step.
- Out of budget → the stuck trade-off, or `blocked` + summary. Never a silent "one more try".

Begin now with the session-start ritual, then SETUP (if needed), then the first eligible step(s).
Report a one-line status after each step; keep prose minimal. When every step in this segment is
passed or a stop condition hits, end with: rows passed/blocked with SHAs and record links,
deferred gates now due, new Learnings, and the exact next action for me.
```

## Caveats (read before running)

1. **S0 bootstraps the harness itself.** Until S0 merges, the scripts, hooks, repo agents for the
   loop and `pustman-v1/main` do not exist. That is why SETUP step 2 runs S0 on `main` and creates
   the integration branch afterwards. The protection hook is not active during S0, so the reviewer's
   test-diff check is the guard for that step.
2. **Review entry points.** S0 copies the skill's agents into `.claude/agents/`, and the repo's own
   agents (`gpui-ui-reviewer`, `ux-auditor`, `http-engine-engineer`, `gpui-test-writer`,
   `rust-architect`) take precedence for their areas. Check in SETUP; don't assume.
3. **Worktree branch point.** Subagent worktrees branch from the default branch (`main`). The
   worker's explicit `git switch -C pustman-v1/S<N> pustman-v1/main` is what guarantees the right
   base. Merges run in the driver's checkout. S0 adds `.claude/worktrees/` to `.gitignore`.
4. **GPUI system libraries.** On this host, `xkbcommon`, `xkbcommon-x11`, `wayland-client`,
   `x11-xcb` and `vulkan` are missing (plan §2), so no `[GPUI]` step can verify until G-DEPS.
   CI (S1) installs them on GitHub runners. Pushing so that CI runs is your action.
5. **No display, no E2E.** A worker or reviewer without a graphical session cannot run
   `cargo run -p pustman` scenarios. Those go to the progress file's deferred gate and are run at
   the next sign-off (S25/S38/S48).
6. **Permissions.** Background subagents surface prompts in the main session, so pre-allow:
   - `cargo *`, `git *`, `gh issue *`, `gh label *`, `pkg-config *`
   - `scripts/pustman-v1-*.sh`, `scripts/promote-check.sh`

   Otherwise an unattended run stalls.
7. **`/goal` mechanics.** The evaluator reads only the transcript, which is why the prompt pastes
   status rows and `VERIFY OK` each turn. Several turns without tool use pause the loop; that is a
   stuck signal.
8. **Concurrency.** Phase 1 allows up to 4 parallel steps (S2/S6/S7 after S1; then S3/S4/S5).
   Phases 2–5 are mostly serial chains plus the hotspot lane, so expect 1–3 in flight.

## Agent-teams variant (experimental, interactive only)

With `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`, the same prompt can run as a **lead** with 3
teammates.

- **Task list:** one task per step S0..S48, with `addBlockedBy` from the graph. G-DEPS and the sign-offs are tasks only the lead completes, after your "go".
- **Isolation:** teammates are not worktree-isolated. Each one creates `pustman-v1/S<N>` in its own `git worktree add`.
- **Hooks:** add a `TaskCompleted` hook that exits 2 unless the task's step record exists.
- **Reviewers** stay separate subagents.
- **When it's worth it:** only in Phase 4, where request_editor ↔ response_viewer ↔ console steps benefit from talking. Elsewhere the graph is too serial.

## Optional: PR-board mode

Instead of squash-merging locally, each step can become a GitHub issue whose body is the step's
prompt, and each passed step a PR into `pustman-v1/main`, babysat with `/loop`. This requires your
explicit permission to push and open PRs. Pick one runner per change.
