# Pustman — Agent Guide

Pustman is a native, GPU-accelerated API client (a Rust port of the Postman workflow) built on
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework behind the Zed editor.
Zed is our reference implementation for "how a large Rust desktop app should be built". When unsure how
to structure something, find the equivalent in Zed and follow its shape — but write our own code
(see [Licensing](#licensing)).

> Status: the repository is **pre-scaffold**. The crate layout below is the *proposed* target, not
> existing code. Update this file when reality diverges from it.

## Where things live

| Need                                   | Go to                                                    |
| -------------------------------------- | -------------------------------------------------------- |
| Building any UI (views, components, styling, layout, Postman screens) | `.agents/skills/rust-gpui-ui-ux/`      |
| Writing GPUI tests                     | `.agents/skills/gpui-testing/`                           |
| Adding a crate / feature end to end    | `.agents/skills/pustman-feature-crate/`                  |
| Actions, keybindings, focus            | `.agents/skills/gpui-actions-keymaps/`                   |
| Settings + persistence                 | `.agents/skills/pustman-settings-persistence/`           |
| HTTP engine, variables, async          | `.agents/skills/pustman-http-engine/`                    |
| Implementing UI from a Figma design    | `.agents/skills/figma-to-gpui/`                          |
| Syncing Figma variables into the theme | `.agents/skills/figma-tokens-to-theme/`                  |
| Path-scoped rules                      | `.agents/rules/`                                         |
| Review / design agent roles            | `.agents/agents/`                                        |

`.agents/` is the harness-neutral source of truth. Skills follow the open `SKILL.md` format (`name` +
`description` frontmatter). **Before editing a file, read every rule in `.agents/rules/` whose `paths:`
globs match it** — harnesses that don't auto-load path-scoped rules must do this by hand. Agent roles in
`.agents/agents/` are prompts: run one as a subagent if your harness has them, or adopt it yourself.
Harness adapters point into `.agents/` and hold no content of their own: `.claude/skills/*` and
`.claude/rules/*` are relative symlinks; `.claude/agents/*.md` are thin wrappers with Claude-only
frontmatter (tools, model, preloaded skills). Edit `.agents/`, never the adapters.

## Toolchain & dependencies

- Rust **1.98.1**, `edition = "2024"`, rustfmt `style_edition = "2024"` (pin in `rust-toolchain.toml`).
- GPUI is consumed as a **git dependency pinned to one Zed revision**. All patterns and snippets in this
  repo were verified against Zed commit `58849026ec30fb225dc5d07cccf37e8408b85555` (2026-11-17).
  `gpui` 0.2.2 is on crates.io, but `gpui_platform` and `gpui_tokio` are not, so pin all three together:

  ```toml
  [workspace.dependencies]
  gpui          = { git = "https://github.com/zed-industries/zed", rev = "58849026ec30fb225dc5d07cccf37e8408b85555" }
  gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "58849026ec30fb225dc5d07cccf37e8408b85555", features = ["font-kit", "wayland", "x11"] }
  gpui_tokio    = { git = "https://github.com/zed-industries/zed", rev = "58849026ec30fb225dc5d07cccf37e8408b85555" }
  ```

  Bump the rev deliberately, in its own commit, and re-check the skills' API notes when you do.
- Linux build needs system packages for `xkbcommon`, `xkbcommon-x11`, `wayland-client`, `x11-xcb`,
  `vulkan`, `fontconfig`, `freetype2` (see Zed's `script/linux` for the canonical list).
- Reference source: `git clone --depth 1 https://github.com/zed-industries/zed` then
  `git -C zed fetch --depth 1 origin 58849026ec30fb225dc5d07cccf37e8408b85555 && git -C zed checkout FETCH_HEAD`.

## Commands

```sh
cargo check --workspace
cargo test --workspace                       # GPUI tests are deterministic; see gpui-testing skill
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p pustman
```

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
assets/
  keymaps/default-{macos,linux,windows}.json
  settings/default.json
  themes/*.json
  icons/*.svg           # 16x16 Lucide-derived
```

Every crate exposes `pub fn init(cx: &mut App)`; `main.rs` calls them in dependency order:
`settings → theme → ui → workspace → feature crates`.

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

### Style
- Comments explain non-obvious *why*; never narrate *what*.
- Full-word names (`request`, not `req`; `response`, not `resp`).
- No `mod.rs`. Library roots are `src/<crate_name>.rs` set via `[lib] path`.
- Prefer editing existing files to adding new ones; prefer adding to an existing crate over a new one
  unless the feature is independently testable.
- `[lints] workspace = true` in every crate.

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
