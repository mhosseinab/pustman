# Pustman v1 — Implementation Plan

**Status: READY (2026-11-17).** Source of requirements: [`PRD.md`](PRD.md). The steps (the *how*) are in
[`pustman-v1-implementation-steps.md`](pustman-v1-implementation-steps.md). The driver is
[`pustman-v1-orchestrator-prompt.md`](pustman-v1-orchestrator-prompt.md). Durable state is in
[`pustman-v1-progress.md`](pustman-v1-progress.md).

## 1. Why, scope, and what does NOT change

**End state of this plan.** We build Pustman from an empty repository through PRD milestones
**M0 Shell → M1 HTTP core + debugging → M2 Workspaces + sharing**. Every step is fully specified,
and at the end Pustman is a usable HTTP client with Git-shareable workspaces on Linux and macOS.
M3–M7 (GraphQL/SSE/advanced auth, WebSocket + Socket.IO, gRPC, MQTT, hardening) appear as **stub
milestones**: each has its goal, crates, depends-on and Verify source. Each is expanded into full
steps at a human expansion gate once the milestone before it is signed off. Detail written today
for M5 would be wrong by the time M5 starts.

**Why now.** The repository has its agent guide, skills, rules and PRD, but no code. The PRD
decisions are fresh. Building the domain crates first, while GPUI system dependencies are still
being installed, gives a verified foundation before any UI exists.

**What does NOT change:**
- `AGENTS.md` rules (correctness, GPUI, async, style, security, licensing). Steps restate them and
  never relax them.
- The pinned Zed rev for `gpui`, `gpui_platform` and `gpui_tokio` (`Cargo.toml`). It moved from `58849026` to Zed
  main `0cc1ffb6b324165ec173a016c5548520c1eeb7ec` (2026-11-19) by human decision, in its own commit. Steps never
  bump it; S1's Verify still greps the old rev and is historical.
- The PRD's non-goals (PRD §2): no scripting runtime, no cloud sync, no mock servers, no runner.
- No Zed GPL code is copied into the repo. We study its patterns only.

## 2. Current state (verified 2026-11-17)

The repository is **greenfield**. Nothing below is code yet. Claims are grounded in the guide files:

| Fact | Source |
| --- | --- |
| Status is pre-scaffold, and the crate layout is *proposed* | `AGENTS.md:9-10` |
| Toolchain is Rust 1.98.1, edition 2024, with the GPUI git deps pinned together | `AGENTS.md:27-45` |
| Commands are check, test, clippy `-D warnings`, fmt check and run | `AGENTS.md:47-58` |
| Proposed crates, and the `init(cx)` order settings → theme → ui → workspace → features | `AGENTS.md:60-91` |
| Rules: no unwrap/expect, errors reach the user, Task/Subscription lifetimes, no foreground IO | `AGENTS.md:93-139` |
| Security: no logging of secrets, keychain for secrets, TLS on by default | `AGENTS.md:131-139` |
| Licensing: Apache GPUI crates are allowed; Zed `ui`/`theme`/`workspace`/... are GPL and must not be copied | `AGENTS.md:141-147` |
| Test file layout is `**/tests/**/*.rs`, `**/*_tests.rs`, `**/test_support.rs` | `.claude/rules/tests.md:1-6` |
| Manifests take deps from `[workspace.dependencies]`; domain crates never depend on UI crates | `.claude/rules/cargo-manifests.md` |
| `http_engine` runs reqwest via `gpui_tokio`; `FakeTransport` runs on GPUI's deterministic executor | `.claude/skills/pustman-http-engine/SKILL.md:48-54` |
| The settings-persistence skill still calls v2.1 the on-disk format, which the PRD supersedes | `PRD.md` §6 note; `.claude/skills/pustman-settings-persistence/SKILL.md` |
| Repo-native agents: gpui-test-writer, gpui-ui-reviewer, http-engine-engineer, rust-architect, ux-auditor | `.claude/agents/` |

**Harness facts (probed 2026-11-17 on the dev machine):**

| Probe | Result | Consequence |
| --- | --- | --- |
| `rustc --version` | `1.98.1` | toolchain OK |
| `pkg-config` xkbcommon, xkbcommon-x11, wayland-client, x11-xcb, vulkan | **MISSING** | GPUI crates cannot build here. See Gate G-DEPS |
| `pkg-config` fontconfig, freetype2, sqlite3, dbus-1, openssl | OK | storage, keyring and TLS deps are fine |
| `gh auth status` | logged in; `origin` = GitHub | LOW findings become `gh issue`s |
| Cargo workspace, CI, lockfile, scripts | **none** | S0 and S1 create the toolchain |
| `.claude/settings.json` hooks | none | S0 adds the protection hooks |
| Deploy target | desktop app; no server, no staging | promotion = release tag + release build + manual smoke (§10) |
| Flag mechanism | none | steps are **additive-only**; no flag framework is invented |

**Which Verifies cannot run here until G-DEPS clears:** every step tagged **[GPUI]** in the steps doc.
That covers `http_engine` (it depends on `gpui`/`gpui_tokio`) and every UI crate. The GPUI-free
domain crates (`api_model`, `variables`, `workspace_store`, `storage`, `secrets`) and their Verifies
run now. The graph orders them first.

## 3. Before → After

| | Before | After this plan (M0–M2) |
| --- | --- | --- |
| Code | none | Cargo workspace with ~16 crates, `cargo run -p pustman` launches |
| Verification | none | CI on Linux + macOS (fmt, clippy `-D warnings`, test); tripwire; protection hooks |
| Product | PRD only | HTTP requests with variables, auth (Basic/Bearer/API key), cookies, cURL, response viewer, timeline, Console, history; Git-shareable folder workspaces with secrets in the keychain; Postman v2.1 import/export |
| Later | — | M3–M7 expanded at gates into steps of the same shape |

## 4. Decided forks

| # | Fork | Decision | Why |
| --- | --- | --- | --- |
| D1 | Workspace file format: JSON or YAML | **JSON** (pretty, 2-space, sorted maps, trailing newline) | Matches `serde_json` already in use and Postman import/export. Deterministic output is simpler. ADR-0001 is written in S0. *The user may veto before S5 starts.* |
| D2 | `reqwest` timing phases | **Spike inside S12.** Measure what `reqwest` exposes. Phases that cannot be measured are **omitted, never estimated** (REQ-NET-01). A hyper-level connector is allowed only inside `ReqwestTransport`. ADR-0003 records the outcome. | The outcome cannot block anything: the UI renders whatever phases exist. |
| D3 | Socket.IO versions | v4 only, unless the library supports more | Applies in M4 (stub) |
| D4 | Scripting | None in v1. Imported scripts are preserved verbatim and never run | PRD §2, AGENTS.md Security |
| D5 | Sync service | Deferred | PRD §6.5 |
| D6 | SQLite library: `rusqlite` or `sqlx` | **`rusqlite`** (bundled feature) behind sync functions called from `cx.background_spawn` | No async runtime is needed for local storage. It is smaller and lets tests use temp DBs. ADR-0002 is written in S0. *The user may veto before S6 starts.* |
| D7 | Request enum growth | `api_model::Request` starts with `Http` only. Each later milestone adds its variant **additively**, with `#[serde(tag = "type")]` so unknown types round-trip as `Unsupported(raw)` | Keeps M0–M2 small. The enum is a hotspot owned by one step per milestone |
| D8 | Where shared redaction lives | `api_model::redact`: pure, no IO; the one `redact()` used by history, Console and export | NFR "one shared `redact()`" |
| D9 | Test placement | `#[cfg(test)] #[path = "<file>_tests.rs"] mod tests;`. **No inline test bodies** in `src/*.rs`, so the protection hook can tell test files apart by path. Integration tests go in `crates/<c>/tests/` | `.claude/rules/tests.md` globs; makes "existing tests are read-only" mechanically enforceable |
| D10 | Action definitions | One `pustman_actions` crate declares every PRD action up front, with the complete default keymaps (Zed's `zed_actions` pattern). Feature crates implement handlers | Removes keymap JSON as a hotspot |
| D11 | Window cap + promotions | Pause at unapplied deploy deltas ≥ 3, or at **unpromoted steps ≥ 16**. Four tags: `release/pustman-v1-1` after P1 (S0–S10, domain-only; smoke = full verify, no app) at G-DEPS; `-2` at M0 (S25); `-3` at M1 (S38); `-4` at M2 (S48). A `codified` delta whose effect is automatic and exercised by the full verify (S0 hooks active in the checkout; S6 migrations applied by tests on a fresh DB) is marked `applied` by the integrator at merge, with that evidence. S1's CI delta is applied when the human pushes and CI is green; S0's manual delta is applied at G-DEPS | A desktop "promotion" is a milestone release. Milestones are 10–16 steps, so 8 would pause mid-milestone with nothing to ship. P1 gets its own tag so S0–S25 never exceeds the 16-step window |

## 5. Interfaces (target shape; workers own the details)

```rust
// api_model (no IO, no GPUI)
pub struct ItemId(ulid::Ulid);
pub enum Request { Http(HttpRequest) /* M3+: Graphql, WebSocket, SocketIo, Grpc, Mqtt */, Unsupported(serde_json::Value) }
pub enum Auth { Inherit, None, Basic{..}, Bearer{..}, ApiKey{..}, Unsupported(serde_json::Value) }
pub struct Variable { key, value: Option<String>, kind: VariableKind /* Default|Secret */, enabled, description }
pub fn redact(text: &str, secrets: &SecretValues) -> String;

// variables
pub fn resolve(template: &str, scopes: &ScopeStack, dynamic: &mut DynamicValues) -> Resolved; // Resolved { text, unresolved: Vec<String>, secrets_used: Vec<String> }

// workspace_store
pub fn open(root: &Path) -> Result<WorkspaceSnapshot, OpenError>;
pub fn save_item(root: &Path, item: &Item) -> Result<PathBuf, SaveError>; // atomic, deterministic
pub fn watch(root: &Path) -> (Watcher, Receiver<FsChange>);

// storage (rusqlite; sync API, called from background tasks)
pub struct LocalStore; impl LocalStore { fn open(path) -> Result<Self>; fn migrate(&self) -> Result<()>; /* history, tabs, layout, local_values, cookies, kv */ }

// secrets
pub trait SecretStore: Send + Sync { fn get(&self, key: &SecretKey) -> Result<Option<String>>; fn set(..); fn delete(..); }
// KeyringStore (OS keychain) + MemorySecretStore (tests)

// http_engine  [GPUI]
pub trait Transport: Send + Sync { fn send(&self, request: ResolvedRequest, settings: RequestSettings) -> BoxFuture<Result<Response, TransportError>>; }
pub enum TransportError { InvalidUrl, Dns, Connect, Tls, Timeout, TooManyRedirects{chain}, Proxy, Protocol, Io, FileNotFound, UnresolvedSecret, Cancelled }
```

## 6. Phases and acceptance

| Phase | Steps | Milestone | Acceptance |
| --- | --- | --- | --- |
| P0 Harness | S0 | — | bootstrap, tripwire, hooks, agents, promotion tooling, `pustman-v1/main` |
| P1 Domain (GPUI-free) | S1–S10 | M0/M1/M2 foundations | `cargo test` green per crate on this machine |
| **G-DEPS** (human) | — | — | install the GPUI libs and `scripts/pustman-v1-init.sh --require-gpui` exits 0 (S0's delta applied); push `pustman-v1/main` with CI green (S1's delta applied), or waive that line in writing; then promote `release/pustman-v1-1` (domain-only, no app smoke). Installing first matters: promote-check's env validator *is* `--require-gpui` |
| P2 HTTP engine [GPUI] | S11–S16 | M1 | engine ACs pass against `FakeTransport` and a loopback server |
| P3 App shell [GPUI] | S17–S24 | **M0** | launch < 1 s, tabs/layout restore → **S25 M0 sign-off** |
| P4 HTTP UI + debugging [GPUI] | S26–S37 | **M1** | HTTP ACs + redaction test (S37) → **S38 M1 sign-off** |
| P5 Workspaces + sharing [GPUI] | S39–S47 | **M2** | sharing AC with two clones (S47) → **S48 M2 sign-off** |
| P6+ | STUB-M3 … STUB-M7 | M3–M7 | expanded at a human gate after the previous milestone's sign-off |

M0's sign-off comes after P2 in the graph only because P2 is independent of the shell. S25 does not
depend on P2. M4, M5 and M6 can be expanded and run in parallel once M2 is signed off (PRD §11).

## 7. Decommission

None. This plan is greenfield and additive.

## 8. Rule and doc updates (made in S0)

- `AGENTS.md`: replace "pre-scaffold" with the real crate list (PRD §9 crates: `workspace_store`,
  `secrets`, `console`, `git`, `pustman_actions`, protocol crates for later milestones). Add
  `hotspots:`, squash-merge `S<N>: <title>`, tags `release/pustman-v1-<n>`, the test-placement rule
  (D9) and "docs ship only with the code they describe".
- `.claude/skills/pustman-settings-persistence/SKILL.md`: v2.1 is the **import/export** format
  only. The on-disk format is the PRD §6.1 folder of files.
- `docs/ARCHITECTURE.md` and `docs/decisions/ADR-0001..0002` (D1, D6).

## 9. Security

- Secrets (secret variables, OAuth tokens, PFX passphrases, proxy passwords) live **only** in the OS
  keychain (`secrets` crate). Workspace files store `{key, kind: "secret", description}` only
  (REQ-VAR-SHARE-02).
- One `redact()` (D8) is applied before history storage, Console raw text and any export.
- Required **redaction test** (S37): a fixture with secrets in every field, then assert that no
  secret value appears in workspace files, in the SQLite file bytes, or in Console exported text.
  The HAR part is added when HAR export lands (M7).
- Imported files are untrusted: 50 MB size limit, nesting depth limit, scripts are never run
  (REQ-IO-06).
- TLS verification is on by default; disabling it is per request and visibly flagged (REQ-HTTP-10).
- Tripwire (S0) is a grep count of `unwrap()`/`expect(`/`std::process::Command`/`from_reader`/
  `let _ =` in non-test `crates/*/src`. Baseline 0; it never grows.

## 10. Risks and rollback

| Risk | Mitigation |
| --- | --- |
| GPUI deps unavailable, so [GPUI] steps cannot verify | G-DEPS is a hard human gate. P1 proceeds meanwhile |
| GPUI API differs from the skills' notes at the pinned rev | Workers read the pinned Zed source (AGENTS.md reference-source command). A mismatch becomes a `Learning:` and the skill is fixed in the same step |
| `reqwest` exposes too few timing phases | D2: omit those phases; a connector inside `ReqwestTransport` only |
| Keychain unavailable in CI or headless Linux | `SecretStore` trait with `MemorySecretStore` in tests; live keyring test is `#[ignore]` and run in sign-off |
| Hotspot contention on `crates/pustman/{Cargo.toml,src/main.rs}` | Serialized by the driver; 9 of 49 steps touch it (18%) |
| Scope creep into M3+ | Stubs are undispatchable; expansion needs a human "go" |

**Rollback:** each step is one squash commit on `pustman-v1/main`, so revert that commit. A release is a
tag (`release/pustman-v1-<n>`), and rolling back means rebuilding the previous tag.

**Promotion for a desktop app** (this replaces staging/prod): `promote-check` with these items:
- full verify in a temp worktree
- the env validator becomes `scripts/pustman-v1-init.sh --require-gpui` plus the keychain probe on the build host
- tripwire
- no existing test modified
- every `crates/storage/migrations/*` has a `down`
- deltas applied
- tag ancestry
- doc-truth
- manual deltas ≤ 2

Then the tag, then `cargo build --release -p pustman`, then the human runs a manual smoke on Linux and macOS.

## 11. Acceptance criteria (with evidence)

| Milestone | Criterion | Evidence |
| --- | --- | --- |
| M0 | Launch to interactive < 1 s; tabs and layout restored after restart; palette lists every action with binding | `PUSTMAN_STARTUP_TRACE=1 cargo run --release -p pustman` prints `startup_ms=<1000`; S23 restore test; S25 human run-through (Linux + macOS) |
| M1 | PRD §5.1 HTTP ACs (resolved POST shown in Console; 6 redirects → TooManyRedirects with chain; self-signed → TLS error + disable flow + badge; cancel within 50 ms leaves no task) | tests named in S11–S16, S26–S37; S38 human run-through |
| M1 | Redaction test | `scripts/pustman-v1-test.sh integration_tests redaction` → PASS |
| M2 | PRD §6.3 sharing AC (two clones; request appears within 1 s; `api_key` unset; diff contains no secret) | S47 two-clone test; S48 human run-through |
| All | CI green on Linux + macOS; tripwire 0; no existing test modified | CI run URL; `promote-check` |

## 12. Implementation map and sources

- PRD sections are mapped to steps in the steps doc (each step cites its REQ IDs).
- Skills: `rust-gpui-ui-ux` (all UI), `gpui-testing`, `pustman-feature-crate`, `gpui-actions-keymaps`,
  `pustman-settings-persistence`, `pustman-http-engine`.
- Reference implementation: Zed at the pinned rev (patterns only; see Licensing).
