---
paths:
  - "**/Cargo.toml"
---

# Cargo manifest traps

See the `pustman-feature-crate` skill for the template.

- Add every dependency to root `[workspace.dependencies]` first, then `dep.workspace = true` in the crate — versions are set in one place.
- `gpui`, `gpui_platform`, `gpui_tokio` must share the same Zed `rev`. Mixed revs compile two gpuis and fail with confusing type mismatches.
- Do not depend on Zed's GPL crates (`ui`, `theme`, `workspace`, `settings`, `editor`, `picker`, `menu`, `db`, `sqlez`, …) — see AGENTS.md › Licensing.
- `test-support` features must forward to dependencies' `test-support` features, or fakes vanish in downstream tests.
- A domain crate (`api_model`, `variables`, `http_engine`, `storage`) gaining a UI-crate dependency is an architecture violation, not a shortcut.
