# Promotion brief — `release/pustman-v1-1`

| | |
|---|---|
| Tag | `release/pustman-v1-1` (domain-only, plan D11) |
| Candidate | `87930fe` on `pustman-v1/main` |
| Window | S0–S10 (11 steps) |
| promote-check | **PROMOTABLE** — 9/9 PASS at `87930fe` |
| App smoke | **none** — tag -1 is domain-only; its full verify inside promote-check is the evidence |
| Build | **none** — tag -1 is domain-only (no app crate yet) |

## What shipped

| Step | Title | SHA | Record | Review |
|---|---|---|---|---|
| S0 | Harness: bootstrap, test runner, tripwire, hooks, promotion tooling, subagents | `4cc3089` | [S0](../progress/pustman-v1/steps/S0.md) | 1 cycle, PASS |
| S1 | Cargo workspace, toolchain, lints, CI, `api_model` skeleton | `1078d67` | [S1](../progress/pustman-v1/steps/S1.md) | 1 cycle, PASS_WITH_ISSUES (1 LOW → #1) |
| S2 | `api_model` types: collections, folders, requests, auth, variables, settings, examples | `1f77523` | [S2](../progress/pustman-v1/steps/S2.md) | 2 cycles (1 HIGH fixed), PASS |
| S3 | Shared `redact()` (D8) | `f9afc7e` | [S3](../progress/pustman-v1/steps/S3.md) | 6 rounds, 5 fix cycles, PASS_WITH_ISSUES ×2 |
| S4 | `variables` resolver | `ad446b9` | [S4](../progress/pustman-v1/steps/S4.md) | 1 cycle, PASS_WITH_ISSUES (3 LOW → #8/#9/#10) |
| S5 | `workspace_store`: on-disk format, deterministic atomic writes | `b08d9a8` | [S5](../progress/pustman-v1/steps/S5.md) | 3 rounds, 3 fix cycles, PASS |
| S6 | `storage`: SQLite local store with migrations | `a84c5a3` | [S6](../progress/pustman-v1/steps/S6.md) | 3 rounds, 2 fix cycles, PASS |
| S7 | `secrets`: OS keychain behind a trait | `626aa12` | [S7](../progress/pustman-v1/steps/S7.md) | 2 rounds (MEDIUM fixed), PASS |
| S8 | `workspace_store` watcher: external changes and problems | `c10350a` | [S8](../progress/pustman-v1/steps/S8.md) | 3 rounds, 3 fix cycles (3 HIGH data-loss bugs), PASS |
| S9 | Postman v2.1/v2.0 import, v2.1 export, environment/globals import | `9edcf20` | [S9](../progress/pustman-v1/steps/S9.md) | 2 rounds, 2 fix cycles, PASS_WITH_ISSUES ×2 |
| S10 | cURL import and export | `55ca3da` | (this window) | BUILD→REVIEW, PASS |

Crates landed: `api_model`, `variables`, `workspace_store`, `storage`, `secrets`. Full verify green
at the candidate (`fmt` → `clippy -D warnings` → `cargo test --workspace` → `tripwire 0`).

## Flagged-step ratio — the one thing to weigh

The promotion rule caps flagged steps (secrets/redaction, persistence/migrations, TLS, import of
untrusted files, deletes) at **20% of the window**. This window is **domain-only**, and the domain
steps *are* the flagged ones:

| category | steps |
|---|---|
| secrets / redaction | S3, S7 |
| persistence / migrations | S5, S6, S8 |
| import of untrusted files | S9, S10 |
| TLS / deletes | none in this window (S12, S28 later) |

**6–7 of 11 = 55–64% against a 20% ceiling.** Per the rule this would block the promotion. The
human ruled (option A) that the ratio is a *window-quality* signal scoped to feature windows
(S11+), and that a domain-only tag proceeds on the evidence instead — which is unusually strong
here (see below). Recorded so a later window is judged on its own ratio.

## Evidence behind the call

- **Every high-stakes step was double-reviewed by independent reviewers** with adversarial probes,
  not read-throughs. S3 and S8 alone took 9 review rounds between them.
- **Every fix was proved by a red-check**: the test was shown to fail for the right reason before
  the fix landed, and re-verified by a reviewer running the mutant independently. This caught
  regressions the code looked correct for — S3 shipped and re-broke the same boundary five times,
  and each was caught by a probe, not by reading.
- **`redact()` — the single masking point — is fuzz-clean**: 30k random URL-ish inputs, 0 secret
  leaks, 0 idempotence failures, 0 timeouts, verified by two independent reviewers.
- **Data safety is pinned by tests**: workspace files carry no secret values, migrations all
  reversible, the watcher's own writes never reported as external (and external changes never
  swallowed), untrusted import bounded before parsing with scripts preserved but never executed.
- Full verify is green and the tripwire baseline is 0 (no `unwrap`/`expect`/`let _ =`/inline
  `mod tests` outside tests).

## Not verified

This release establishes **none** of the following:

1. **No application runs.** There is no binary yet — `pustman` arrives at S22. Nothing here has
   been observed on screen, on Linux or macOS.
2. **No GUI, no UX.** Every screen, keyboard path and accessibility claim in the PRD is untested;
   the redesign pack's screen specs (S01–S24) have not been applied to any code.
3. **No real request has been sent.** `http_engine` does not exist yet (S11–S16). Nothing in this
   window opens a socket, resolves a variable at send time, follows a redirect, or exercises TLS.
4. **The OS keychain is unproven on this host.** `secrets` compiles and its memory store is tested,
   but `cargo test -p secrets -- --ignored` (the live keyring round-trip) has not run. Same for
   `live_keyring_roundtrip`'s cleanup guard.
5. **SQLite migrations have never run on real user data.** The reversibility proofs run on fresh
   in-memory databases; no migration has been applied to, or rolled back from, a workspace with
   content.
6. **The watcher has not seen a real git pull.** Its probes were synthetic directory operations;
   inotify's behaviour under a real checkout, a rebase, or a queue overflow is unobserved.
7. **cURL/Postman import has only seen hand-written fixtures.** No real exported Postman collection
   and no real pasted cURL has been through it.
8. **CI has not run on the candidate.** CI was green at `e5347fc`; the candidate is `87930fe`.
   Push and confirm before relying on CI for this tag.
9. **Windows is entirely untested.** Everything here ran on Linux. `secrets`' Windows Credential
   Manager path and the case-insensitive target-name collision noted in S7's record are unverified.
10. **Six open LOW issues** (#1–#11 minus the three closed implicitly) are unfixed by design —
    each is recorded with its file:line and the shape of the fix.
11. **The CARRY-FORWARD stands**: `SecretValues::refused_count()` has no caller yet. The step that
    wires it into History/Console/export must read it and warn the user, or S3's F3 stays half
    closed.
12. **Windows cap / later milestones.** STUB-M3..M7 are unexpanded; GraphQL, WebSocket, gRPC and
    MQTT are absent.

## Release shape

Tag -1 is deliberately domain-only (plan D11): the whole window is the data layer — the model, the
redaction boundary, variables, the on-disk format, SQLite, the keychain, the watcher, and the two
import paths. No build and no app smoke is the designed evidence model for this tag; the full
verify *is* the smoke. Tag -2 (`release/pustman-v1-2`) is the first one that needs
`cargo build --release -p pustman` and a human manual smoke on Linux + macOS.
