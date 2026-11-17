---
name: pustman-feature-crate
description: Scaffold or extend a Pustman workspace crate the Zed way — Cargo.toml layout, lib path, workspace lints, init(cx) registration, domain/UI split, test-support feature, clippy config, and the end-to-end feature checklist (actions, keybindings, settings, theme tokens, persistence, a11y, platforms, docs). Use when creating a new crate, adding a feature that spans crates, wiring a crate into main.rs, or setting up workspace Cargo/clippy/rustfmt/toolchain configuration.
---

# Feature crates

## Decide where code goes
1. Can it live in an existing crate without muddying it? Prefer that.
2. Domain logic (no GPUI views) → domain crate (`api_model`, `variables`, `http_engine`, `storage`).
3. UI → a UI crate depending on the domain crate (`request_editor`, `response_viewer`). When a domain and its UI
   share a name, the UI crate gets a `_ui` suffix (Zed: `git` / `git_ui`).
4. Dependencies only point "down": `pustman → feature UI crates → workspace → ui → theme → settings`, and UI → domain. Never domain → UI.

## Crate template

```toml
# crates/environment_picker/Cargo.toml
[package]
name = "environment_picker"
version = "0.1.0"
edition.workspace = true
publish = false
license = "<project license>"

[lints]
workspace = true

[lib]
path = "src/environment_picker.rs"   # never mod.rs, never lib.rs-by-default ambiguity

[features]
test-support = ["workspace/test-support"]

[dependencies]
anyhow.workspace = true
gpui.workspace = true
ui.workspace = true
workspace.workspace = true
api_model.workspace = true

[dev-dependencies]
gpui = { workspace = true, features = ["test-support"] }
workspace = { workspace = true, features = ["test-support"] }
```

```rust
// crates/environment_picker/src/environment_picker.rs
pub fn init(cx: &mut App) {
    cx.observe_new(EnvironmentPicker::register).detach();
}
```

Then add the crate to root `[workspace] members` and `[workspace.dependencies]` (`environment_picker = { path = "crates/environment_picker" }`)
and call `environment_picker::init(cx)` in `crates/pustman/src/main.rs` after its dependencies.

## Workspace config (root)

```toml
[workspace.package]
edition = "2024"

[workspace.lints.clippy]
dbg_macro = "deny"
todo = "deny"
redundant_clone = "deny"
disallowed_methods = "deny"
declare_interior_mutable_const = "deny"
style = { level = "allow", priority = -1 }   # Zed allows style lints; correctness/suspicious stay on
type_complexity = "allow"
too_many_arguments = "allow"
large_enum_variant = "allow"
```

```toml
# clippy.toml
disallowed-methods = [
  { path = "std::process::Command::spawn",  reason = "Use smol::process::Command so the UI never blocks" },
  { path = "std::process::Command::output", reason = "Use smol::process::Command so the UI never blocks" },
  { path = "std::process::Command::status", reason = "Use smol::process::Command so the UI never blocks" },
  { path = "smol::Timer::after",            reason = "Use BackgroundExecutor::timer so tests can control time" },
  { path = "serde_json::from_reader",       reason = "Slow; read to bytes then serde_json::from_slice" },
]
```

```toml
# rustfmt.toml
style_edition = "2024"
# rust-toolchain.toml
[toolchain]
channel = "1.98.1"
components = ["rustfmt", "clippy"]
```

## End-to-end feature checklist (from Zed's feature process)
- [ ] Actions declared with doc comments; keybindings in all 3 keymaps, conflicts checked (`gpui-actions-keymaps`).
- [ ] Settings added with defaults + schema + docs (`pustman-settings-persistence`).
- [ ] New colors are theme tokens defined in every bundled theme.
- [ ] State that should survive restart is persisted.
- [ ] UI follows `rust-gpui-ui-ux`, UX checklist walked.
- [ ] Tests: behavior tests in the crate, written first (`gpui-testing`).
- [ ] Errors surfaced to the user.
- [ ] Security review if it touches secrets, files, network, or imported data.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean.
