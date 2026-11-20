---
name: lows
description: "Remediate open LOW review issues (GitHub labels pustman-v1 + low) with pi workers: read-only triage, disjoint fix lanes (pi worker → independent pi reviewer, 2 fix cycles), squash-merge each lane onto pustman-v1/main, full verify, close fixed issues. Use for \"fix the lows\", \"/lows S26\", \"/lows #127 #128\"."
argument-hint: "[all | S<N>[,S<M>...] | #<issue>...]"
disable-model-invocation: true
---

`$ARGUMENTS` is what follows the skill command (`/lows …` in Claude Code, `/skill:lows …` in pi, where it
arrives as a request after these instructions). Default `all`. `S<N>` narrows to issues whose title starts
with `S<N>` (`S26` matches `S26a:`, `S26b:` …); `#<n>` names issues.

You are the driver. pi workers write the code and pi reviewers judge it; you triage, plan lanes, merge,
verify and write the progress files. Only you touch `pustman-v1/main`, `docs/progress/**` and GitHub
issues. Never push, tag or release. Run only when no v1 driver merge is mid-flight (one progress writer).

## Dispatching pi

- **Claude Code:** one background Bash call per job, request JSON on stdin from a file — never
  interpolate issue text into a shell command:
  `cd <cwd> && node "$PI" task < $T/<job>.json` with
  `PI=$(ls ~/.claude/plugins/cache/cc-pi/pi/*/scripts/companion.mjs | sort -V | tail -1)` (newest
  installed plugin) and `$T` = the session's job tmp dir. The request:
  `{"cwd": "<worktree>", "model": "xiaomi-token-plan-sgp/mimo-v2.6-pro", "timeoutSeconds": 3600, "prompt": "…"}`;
  read-only jobs add `"tools": ["read","grep","find","ls","bash"]`. The verdict is the result's `"text"`.
  A fix cycle or a timed-out job **resumes** the same session (`"resume": "<job id>"`, id from
  `~/.local/state/cc-pi/*/*/job.json` whose `root` is the worktree) instead of starting a fresh one.
- **pi:** one `subagent` call in `tasks` mode, each task with its `cwd` set to the lane worktree.
- At most **4 jobs at once**, one job per worktree. If v1 build workers already hold slots, lanes wait.

## Steps

0. **Preflight** (main checkout): branch `pustman-v1/main`; `git status --porcelain` empty apart from
   `.claude/agent-memory/`; the last entry of `docs/progress/pustman-v1/log.md` is not an unfinished
   `MERGING`. Record `BASE=$(git rev-parse --short HEAD)`. List unmerged step branches
   (`git branch --list 'pustman-v1/S*' --no-merged pustman-v1/main`) with their tips and worktrees.
1. **Select:** `gh issue list -l pustman-v1 -l low --state open --limit 500 --json number,title,body,comments`,
   narrowed by `$ARGUMENTS`. Skip issues whose last comment is a `lows:` status of `human-only` or
   `won't fix` unless named explicitly; report the skip count.
2. **Triage** (read-only, ≤ 4 pi jobs of ~15 issues each, in a scratch worktree
   `.claude/worktrees/pustman-v1/lows-triage` detached at `BASE`): write the batch's issues to
   `.claude/worktrees/pustman-v1/.briefs/lows-triage-<k>.md`. Each job checks every issue against the
   code at `BASE` and replies with JSON lines
   `{"issue": n, "verdict": "already_fixed|fixable|human_only|wontfix", "files": [...], "reason": "…"}`.
   `human_only` = needs a product/design call or a real host (macOS smoke); `wontfix` = the finding is
   wrong or the fix costs more than it buys — say why. Re-triage any issue missing from the replies.
   Spot-check two `already_fixed` verdicts yourself before trusting them.
3. **Lanes:** group `fixable` issues into lanes of ≤ 6 with pairwise-disjoint file sets (usually one
   crate per lane). Hotspots — `Cargo.lock`, `crates/pustman/src/main.rs`, `crates/pustman/Cargo.toml`,
   `crates/pustman/src/pustman.rs` — belong to at most one lane. An issue whose files live only on an
   unmerged step branch waits for that step to merge; don't build lanes on step branches. Show the lane
   table (lane, issues, files, extra lens), then for each lane:
   `git worktree add .claude/worktrees/pustman-v1/lows-<lane> -b fix/lows-<lane> pustman-v1/main`.
   **Worker brief** `.briefs/lows-<lane>-build.md`: the issues verbatim (number, title, body), the files
   allowed, and these rules: read `AGENTS.md` and the `.agents/rules/*.md` matching each file; reuse
   workspace crates and `[workspace.dependencies]` — never hand-roll what exists (DRY), simplest fix
   (KISS), nothing beyond the issues; a behavior fix comes with a failing-first test in
   `src/<file>_tests.rs` (no inline `mod tests {`); **existing test files and
   `docs/pustman-v1-implementation-steps.md` are read-only** — a finding about an existing test is
   answered by a new test or reported `not_fixable`; no `unwrap`/`expect`/`let _ =` outside tests;
   errors reach the user; theme tokens only; never log secrets. Verify: the crate's tests, clippy
   `-D warnings`, `cargo fmt --all -- --check`, `scripts/pustman-v1-tripwire.sh` → `TRIPWIRE 0`. One
   commit per issue `lows: #<n> <title>` staged by name, ending with
   `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; report to
   `.reports/lows-<lane>-build.md` (per issue: fixed `<sha>` / not_fixable + reason, Verify output);
   final line `lows-<lane> DONE <sha>` or `lows-<lane> BLOCKED <reason>`.
4. **Review** each finished lane with a fresh read-only pi job (tools with bash; it must not commit,
   stash or leave changes — `git status --porcelain` clean afterwards). Lenses: **spec conformance**
   (each fix does what its issue asks, no more, files inside the lane) and **test integrity** (new
   tests fail without the fix, no existing test edited/weakened/deleted — check
   `git diff --name-status pustman-v1/main...HEAD -- '*_tests.rs' 'crates/*/tests/**'` shows only
   `A`). Add the **security** lens (`.agents/agents/security-reviewer.md`) for `api_model` redaction,
   `secrets`, `console`, history or cookies. Report `.reports/lows-<lane>-review<N>.md` with
   `[HIGH|MEDIUM|LOW] file:line` findings; reply `lows-<lane> REVIEW<N> <PASS|PASS_WITH_ISSUES|CHANGES_REQUESTED>`.
   MEDIUM/HIGH → resume the worker with the findings; round 2+ reviews check only the prior findings and
   the fix diff. Budget **2 fix cycles**; still MEDIUM/HIGH after that → drop the offending commits
   from the lane (or the whole lane) and mark those issues `partial`.
5. **Integrate**, one lane at a time, in the main checkout:
   `git -C .claude/worktrees/pustman-v1/lows-<lane> rebase pustman-v1/main` (conflict → resolve it
   yourself if trivial, else drop the lane), then `git merge --squash fix/lows-<lane>`, run
   `scripts/pustman-v1-verify.sh` → `VERIFY OK`, and commit `remediation: <lane> LOWs` whose body
   lists `Fixes #<n>` per fixed issue (local trailers only — nothing is pushed) and the
   `Co-Authored-By` line. A red verify you cannot fix in one small `remediation: <short>` commit →
   `git reset --merge` and drop the lane.
6. **Progress commit** `progress: LOW remediation`: one log entry (`BASE`, lane commits, fixed /
   stale / human-only / won't-fix / partial counts, the verify result) and, if a lane changed a
   documented boundary, the matching `docs/ARCHITECTURE.md` line. Stage by name.
7. **Issues:** close fixed ones `gh issue close <n> -c "lows: fixed by <sha> on pustman-v1/main"`,
   stale ones `-c "lows: already fixed at <BASE> — <reason>"`, won't-fix ones with
   `--reason "not planned"` and the why. Comment `lows: <human-only|partial> — <reason>` on the
   ones that stay open. File each reviewer LOW as a new issue `-l pustman-v1 -l low`, titled
   `lows-<lane>: …`.
8. **Clean up:** `git worktree remove` each `lows-*` worktree (after `git status --porcelain` is
   clean there) and `git branch -D fix/lows-<lane>` for merged or dropped lanes.
9. **Report:** a status table — fixed (issue → sha), stale, human-only (with the decision each needs),
   won't fix, partial/still open — plus the lane commits, the verify evidence and new LOWs filed.
