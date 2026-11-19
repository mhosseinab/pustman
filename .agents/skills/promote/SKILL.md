---
name: promote
description: "Prepare and run a promotion of pustman-v1/main — one-page brief, doc-truth check, promote-check gate — then, only on my explicit go, tag release/pustman-v1-<n>, build the release and hand off the manual smoke. Desktop app: there is no deploy. Use for \"promote\", \"promote up to S12\", \"what's promotable?\"."
argument-hint: [sha | S<N> | "check"]
disable-model-invocation: true
allowed-tools: Bash(git *) Bash(scripts/promote-check.sh *) Read Grep Glob Edit
---

`$ARGUMENTS` is what follows the skill command (`/promote …` in Claude Code, `/skill:promote …` in pi, where it arrives as a request after these instructions).

Promotion window: from the last `release/pustman-v1-*` tag to `$ARGUMENTS` (default: `pustman-v1/main` HEAD).
This is a desktop app: there is no staging and no deploy — "staging" means the build host, and a
promotion is a tag plus a human manual smoke on Linux + macOS. You never release the window cap by
changing its numbers, and never tag without my "go". Run only when no execution segment or
orchestrator is running — you write the progress files, and only one writer may touch them at a
time. Before step 1: `git status` must be clean and the log's last entry must not be an unfinished
`MERGING` or dispatch; otherwise stop and ask me whether a run is still in flight.

1. **Prepare, in parallel (fresh subagents** — Claude Code: the Agent tool; pi: one `subagent` call in `tasks` mode**):**
   - `promotion-brief-writer` → `docs/briefs/release-pustman-v1-<n>.md` (one page; flag rate stated;
     "Not verified" section).
   - `doc-truth-checker` → PASS/FAIL with 5 confirmed/refuted claims.
   - `promote-checker` → runs `scripts/promote-check.sh <sha>`; PASS/FAIL per item (hand it the
     doc-truth result for item 8).
2. **Reconcile.** Any FAIL, any refuted doc claim, or flag rate > 20% → report **NOT PROMOTABLE**
   with the exact blockers and stop. Else report **PROMOTABLE**: brief path, the flagged steps to
   deep-read (offer `/explain S<N>` for each), the deploy deltas the human must apply on the build
   hosts (scripted/manual lines verbatim), and wait.
3. **On my "go":** `git tag release/pustman-v1-<n> <sha>`; build the release
   (`cargo build --release -p pustman`); the human runs the manual smoke on Linux and macOS;
   mark the window's deltas `applied(<evidence>)` in `DEPLOY_DELTAS.md`; append the Promotions row in the progress
   main file, a `shipped in release/pustman-v1-<n>` line under *Later* in each included step record, and a
   log entry; commit them as `progress: release/pustman-v1-<n>`; report the smoke result and stop. There is
   no production deploy — when I say "smoke done", the promotion is complete.
4. If `$ARGUMENTS` is `check`: run only step 1's `promote-checker` and report.
