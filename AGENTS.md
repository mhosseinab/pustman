# Pustman — Agent Guide

Pustman is a native, GPU-accelerated API client (a Rust port of the Postman workflow) built on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework behind the Zed editor.
Zed is our reference implementation for "how a large Rust desktop app should be built". When unsure how
to structure something, find the equivalent in Zed and follow its shape — but write our own code
(see [Licensing](#licensing)).

> Status: v1 (M0–M5; M3–M5 expanded early as S49–S71) is under construction — driver and steps in `docs/pustman-v1-plan.md`. The
> crate layout below is the PRD §9 target; **crates appear as the steps land**. The harness
> (bootstrap/test/verify/tripwire scripts, protection hooks, subagents, promotion tooling) exists
> from S0. Update this file when reality diverges from it.

## Where things live

| Need                                   | Go to                                                    |
| -------------------------------------- | -------------------------------------------------------- |
| Building any UI (views, components, styling, layout, Postman screens) | `.agents/skills/rust-gpui-ui-ux/`      |
| UI design reference (screen specs, states, tokens, fixtures) | `docs/postman_redesign_pack/` — how to use it and what wins in a conflict: `rust-gpui-ui-ux` skill, "Design source" |
| Writing GPUI tests                     | `.agents/skills/gpui-testing/`                           |
| Adding a crate / feature end to end    | `.agents/skills/pustman-feature-crate/`                  |
| Actions, keybindings, focus            | `.agents/skills/gpui-actions-keymaps/`                   |
| Settings + persistence                 | `.agents/skills/pustman-settings-persistence/`           |
| HTTP engine, variables, async          | `.agents/skills/pustman-http-engine/`                    |
| Implementing UI from a Figma design    | `.agents/skills/figma-to-gpui/`                          |
| Syncing Figma variables into the theme | `.agents/skills/figma-tokens-to-theme/`                  |
| Path-scoped rules                      | `.agents/rules/`                                         |
| Review / design agent roles            | `.agents/agents/`                                        |
| Fixing open LOW review issues (`/lows`) | `.agents/skills/lows/` (pi workers)                     |

`.agents/` is the harness-neutral source of truth. Skills follow the open `SKILL.md` format (`name` +
`description` frontmatter). **Before editing a file, read every rule in `.agents/rules/` whose `paths:`
globs match it** — harnesses that don't auto-load path-scoped rules must do this by hand. Agent roles in
`.agents/agents/` are prompts: run one as a subagent if your harness has them, or adopt it yourself.
Harness adapters point into `.agents/` and hold no content of their own: `.claude/skills/*` and
`.claude/rules/*` are relative symlinks into `.agents/`. Roles are split: the five repo-native roles
live in `.agents/agents/`, and `.claude/agents/` holds the loop roles S0 copied in as full prompts, plus
Claude-frontmatter variants of the five repo-native roles. Edit `.agents/` for the shared content; edit
`.claude/agents/` only for Claude-specific frontmatter. `.pi/agents/` is generated from `.claude/agents/`
(see [Pi specifics](#pi-specifics)); never edit it by hand.

- For any UI work, load the `rust-gpui-ui-ux` skill before writing code, and follow its "Design source"
  section for `docs/postman_redesign_pack/`.
- Before claiming a UI change works, run the relevant GPUI test and say which checklist items
  (`.agents/skills/rust-gpui-ui-ux/references/ux-checklist.md`) you verified and which you could not.

### Pi specifics

[pi](https://pi.dev/) reads this file (not CLAUDE.md) and discovers `.agents/skills/` itself. The
Claude-only pieces have pi adapters under `.pi/`:

| Claude Code | pi |
| --- | --- |
| `.claude/agents/*.md` | `.pi/agents/*.md`, generated: `bun scripts/pi-sync-agents.ts` (`--check` fails on drift) |
| Agent tool | `subagent` tool, `.pi/extensions/subagent/` (vendored MIT example; defaults to project agents; `cwd` per task) |
| `PreToolUse` protect hook | `.pi/extensions/pustman-guards/` runs the same `.claude/hooks/protect-steps-and-tests.sh` |
| `.claude/rules/` auto-load | the same extension lists `.agents/rules/` and blocks edit/write until the matching rule is read |
| `SessionStart(compact)` reminder | the same extension steers to the progress files after a compaction |
| `/explain`, `/promote`, `/lows` | `/skill:explain`, `/skill:promote`, `/skill:lows` |

- Project extensions and skills load only in a trusted folder: run `/trust` once at the repo root (it
  covers the worktrees under `.claude/worktrees/`). Print mode cannot prompt; untrusted means the guards
  and skills are silently skipped.
- The extensions guard only the edit/write tools, like the Claude hook; pi has no permission prompts or
  sandbox, so `bash` is unguarded.
- Helper tests: `bun test ./.pi/extensions/pustman-guards/rules.test.ts ./scripts/pi-sync-agents.test.ts` (16 tests; a bare `.pi/extensions` filter is skipped as a hidden dir).
- Both harnesses stay supported side by side. The v1 driver has a Claude prompt
  (`docs/pustman-v1-orchestrator-prompt.md`) and a pi prompt (`docs/pustman-v1-orchestrator-prompt-pi.md`,
  which has a "Switching harnesses" section). Never run both drivers at once.

## Toolchain & dependencies

- Rust **1.98.1**, `edition = "2024"`, rustfmt `style_edition = "2024"` (toolchain pinned in `rust-toolchain.toml`; `style_edition` in `rustfmt.toml`).
- GPUI is consumed as a **git dependency pinned to one Zed revision**. All patterns and snippets in this
  repo were verified against Zed commit `0cc1ffb6b324165ec173a016c5548520c1eeb7ec` (Zed main, 2026-11-19).
  `gpui` 0.2.2 is on crates.io, but `gpui_platform` and `gpui_tokio` are not, so pin all three together:

  ```toml
  [workspace.dependencies]
  gpui          = { git = "https://github.com/zed-industries/zed", rev = "0cc1ffb6b324165ec173a016c5548520c1eeb7ec" }
  gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "0cc1ffb6b324165ec173a016c5548520c1eeb7ec", features = ["font-kit", "wayland", "x11"] }
  gpui_tokio    = { git = "https://github.com/zed-industries/zed", rev = "0cc1ffb6b324165ec173a016c5548520c1eeb7ec" }
  ```

  Bump the rev deliberately, in its own commit, and re-check the skills' API notes when you do.
- Linux build needs system packages for `xkbcommon`, `xkbcommon-x11`, `wayland-client`, `x11-xcb`,
  `vulkan`, `fontconfig`, `freetype2` (see Zed's `script/linux` for the canonical list).
- Reference source: `git clone --depth 1 https://github.com/zed-industries/zed` then
  `git -C zed fetch --depth 1 origin 0cc1ffb6b324165ec173a016c5548520c1eeb7ec && git -C zed checkout FETCH_HEAD`.

## Commands

```sh
cargo check --workspace
cargo test --workspace                       # GPUI tests are deterministic; see gpui-testing skill
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p pustman
```

Cargo uses at most 3 cores: the checked-in `.cargo/config.toml` sets `[build] jobs = 3` and
`RUST_TEST_THREADS=3` for every checkout and worktree. Don't raise it with `-j`, `--jobs` or `--test-threads`.

Run the narrowest command that proves your change (`cargo test -p request_editor some_test`) before the
workspace-wide ones.

## Proposed architecture

One crate per feature, Zed style. Domain crates have no GPUI views; UI crates depend on domain crates,
never the reverse.

```
crates/
  pustman/              # binary: main.rs, init order, window setup, app menus
  ui/                   # OUR design system: tokens, components, prelude (original code, not Zed's GPL ui)
  theme/                # Theme, ThemeColors, StatusColors, light/dark JSON themes, ActiveTheme trait
  settings/             # SettingsStore, typed settings, assets/settings/default.json
  workspace/            # Workspace, Pane, Item, Dock, Panel, ModalLayer, toasts, status bar
  text_input/           # single + multi-line text editing (EntityInputHandler), syntax-free
  api_model/            # Request, Collection, Folder, Environment, Variable, Auth; Postman v2.1 import/export
  variables/            # {{var}} resolution, scopes, narrowest wins (precedence in the pustman-http-engine skill)
  http_engine/          # send/cancel requests (reqwest on gpui_tokio), timing, cookies, redirects
  storage/              # SQLite persistence (history, open tabs, UI state), key-value store
  collections_panel/    # left dock: tree of collections/folders/requests
  request_editor/       # Item: URL bar, method, params/headers/body/auth tabs
  response_viewer/      # status/time/size, body (pretty/raw/preview), headers, cookies
  environment_picker/   # PickerDelegate for active environment
  command_palette/      # action search
  history_panel/        # past requests
  workspace_store/      # on-disk workspace format (PRD §6.1): deterministic JSON, atomic writes, watcher
  secrets/              # OS keychain SecretStore (KeyringStore; MemorySecretStore in tests)
  console/              # console entries, ring buffer, HAR export, redaction
  git/                  # thin git CLI wrapper (smol::process): status, diff, commit, pull, push, branch
  pustman_actions/      # every PRD action + default keymaps (D10); feature crates implement handlers
  integration_tests/    # end-to-end proofs (redaction S37, sharing AC S47)
assets/
  keymaps/default-{macos,linux,windows}.json
  settings/default.json
  themes/*.json
  icons/*.svg           # 16x16 Lucide-derived
```

Every **UI** crate exposes `pub fn init(cx: &mut App)` (domain crates expose plain functions and types); `main.rs` calls them in dependency order:
`settings → theme → ui → workspace → feature crates`.

Later milestones add `websocket_client` (M4), `grpc_client` (M5) and `mqtt_client` (M6) — protocol
crates with no GPUI views, sharing the `SessionEvent` shape from `docs/PRD.md` §9.

## Hotspots (the v1 driver serializes these)

- `Cargo.lock` — regenerated at merge, never hand-merged.
- `crates/pustman/src/main.rs` — init order; 9 of 49 v1 steps touch it.
- `crates/pustman/Cargo.toml` — the same steps as `main.rs`.

## Git & releases (v1)

- Integration branch `pustman-v1/main`; each step lands as one squash commit `S<N>: <title>`.
- Step worktrees live at `.claude/worktrees/pustman-v1/S<N>` (gitignored, survives reboots; never `/tmp`), one
  worktree and one branch `pustman-v1/S<N>` per step. Work only inside your own worktree; never touch another
  worktree, the main checkout or `pustman-v1/main`. Worker briefs go in `.claude/worktrees/pustman-v1/.briefs/`.
- Run cargo from inside your worktree and let it use the worktree's own `target/`; don't set `CARGO_TARGET_DIR`
  to a shared path (parallel builds then block on one cache).
- Each worktree's `target/` costs 4–10 GB, so the driver runs `scripts/pustman-v1-cleanup.sh` after every
  merge (removes merged steps' worktrees, keeps branches) and before every dispatch (fails under 80 GB free; 15 GB in a cloud session, where `CLAUDE_CODE_REMOTE=true`).
  It also deletes stale test binaries in every `target/debug/deps` (older builds of the same name).
  Test: `bun test ./scripts/pustman-v1-cleanup.test.ts ./scripts/pustman-v1-cleanup-prune.test.ts`.
- Releases are tags `release/pustman-v1-<n>` (plan D11). This is a desktop app — "staging" is the
  build host: tag → `cargo build --release -p pustman` → package with the app icon → human manual smoke on
  Linux and macOS (exact steps: the `promote` skill, step 3).
- Host/env changes are typed Deploy deltas in `DEPLOY_DELTAS.md` (`codified`/`scripted`/`manual`);
  `scripts/promote-check.sh` refuses a promotion with pending deltas.

## Rules (traps, not maps)

These are the mistakes that actually bite in GPUI codebases. Zed enforces most with custom lints.

### Correctness
- Prioritise correctness and clarity over speed of delivery or cleverness.
- No `unwrap()`/`expect()` in non-test code, no panicking indexing (`vec[i]`) on data you don't control —
  use `.get(i)`, `?`, or a match.
- Never `let _ = fallible();`. Propagate with `?`, log with `.log_err()`, or handle explicitly.
- **Errors must reach the user.** A failed send, import, or save shows a toast/notification or inline
  error; it is never only logged. Async UI work ends in `detach_and_log_err(cx)` at minimum, preferably a
  user-visible notification.

### GPUI
- Never call `cx.notify()` or `Entity::update` inside `render`. Render is a pure function of state.
- Call `cx.notify()` after mutating state that affects rendering.
- Inside `entity.update(cx, |this, cx| …)` use the **inner** `cx`; never re-enter `update` on the same
  entity (it panics).
- **Dropping a `Task` cancels it.** Await it, `.detach()`/`.detach_and_log_err(cx)`, or store it in a
  field (e.g. `pending_send: Option<Task<()>>` — replacing it cancels the old request, which is what we want).
- Store `Subscription`s (`_subscriptions: Vec<Subscription>`); a dropped subscription stops firing.
- No blocking IO on the foreground thread: file, DB and network work go through `cx.background_spawn`
  or `gpui_tokio::Tokio::spawn`.
- String literals into `SharedString` via `SharedString::from_static("…")`; owned strings via `.into()`.
- Global element IDs must be unique per frame — derive IDs from stable model IDs, not indices that shift.
- Every interactive element is keyboard-reachable and has an accessible label.

### Async & concurrency
- Clone into async blocks with a shadowing block:
  `cx.spawn({ let client = self.client.clone(); async move |this, cx| { … } })`.
- In tests use GPUI timers (`cx.background_executor().timer(d)`), never `smol::Timer` or `std::thread::sleep`.
- Spawn processes with `smol::process`, never `std::process::Command::{spawn,output,status}`.
- Parse JSON with `serde_json::from_slice`, not `from_reader`.

### Reuse — SOLID, KISS, DRY
- **Never re-implement what already exists.** Before writing a helper, search the workspace crates and
  root `[workspace.dependencies]`; if either provides it, use it. Hand-rolling a codec, parser, encoder
  or utility we already depend on (e.g. base64 instead of the `base64` crate) is a review finding, not a
  scope workaround.
- Adding `dep.workspace = true` for an existing workspace dependency to the crate you are editing is
  always in scope, even when a step's Edits line does not name that `Cargo.toml`.
- DRY applies to meaning: one concept, one implementation; call it, don't copy it. KISS: the simplest
  code that meets the requirement, no speculative abstraction. SOLID as design pressure: small,
  single-purpose types and functions; depend on stable boundaries.

### Style
- Comments explain non-obvious *why*; never narrate *what*.
- Full-word names (`request`, not `req`; `response`, not `resp`).
- No `mod.rs`. Library roots are `src/<crate_name>.rs` set via `[lib] path`.
- Prefer editing existing files to adding new ones; prefer adding to an existing crate over a new one
  unless the feature is independently testable.
- `[lints] workspace = true` in every crate.

## Test placement (D9)

- Unit tests live next to their code as `src/<file>_tests.rs`, wired with
  `#[cfg(test)] #[path = "<file>_tests.rs"] mod tests;`. **No inline `mod tests {` in `src/*.rs`** —
  `scripts/pustman-v1-tripwire.sh` counts it, and the path convention is what makes "existing tests
  are read-only" mechanically enforceable.
- Integration tests live in `crates/<crate>/tests/`; shared helpers in `src/test_support.rs`. A later
  step needing more helpers adds a NEW file — it never edits an earlier step's helper.
- Test names are behavior sentences (`test_unresolved_secret_never_sends_an_empty_string`).

## Docs

Docs ship only with the code they describe: a doc edit lands in the same commit as the change it
describes (e.g. `docs/ARCHITECTURE.md` gains the crate it maps). `docs/pustman-v1-implementation-steps.md`
and existing test files are read-only for workers — enforced by `.claude/hooks/protect-steps-and-tests.sh`.
The only exception is a human authorization: the driver records it as an `open` row in
`docs/progress/pustman-v1/test-edit-authorizations.tsv` on `pustman-v1/main` (quoting the human), the
hook then allows that one file, and the row becomes `used` with the commit once the edit lands.
`scripts/promote-check.sh` item 4 fails any test file changed in the release window without a row.
Never edit an existing test past the hook any other way.

## Security (it's an API client — it handles secrets)
- Never log request/response bodies, `Authorization` headers, cookies, or variable values marked secret.
- Secret variables are stored via the OS keychain, not in plain SQLite/JSON.
- TLS verification is on by default; disabling it is a per-request, visibly-flagged setting.
- Imported collections are untrusted data: no script execution without explicit user opt-in.

## Licensing

GPUI (`gpui`, `gpui_platform`, `gpui_tokio`), `http_client`, `reqwest_client`, `collections`, `util`,
`refineable` are **Apache-2.0** and may be depended on. Zed's `ui`, `theme`, `workspace`, `settings`,
`component`, `picker`, `menu`, `ui_input`, `editor`, `db`, `sqlez` are **GPL-3.0**: we study their
*patterns* and write our own implementations. Do not paste GPL code into this repo unless the project
explicitly adopts GPL-3.0.

## Rule hygiene

Add a rule here or in `.agents/rules/` only if it is non-obvious, has bitten us more than once, and is
actionable. Rules describe traps to avoid, not maps of the code. State each fact in exactly one place and
link to it from elsewhere.
