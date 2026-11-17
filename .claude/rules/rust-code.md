---
paths:
  - "**/*.rs"
---

# Rust code traps

- Inline `#[cfg(test)] mod tests` blocks are tests too: load the `gpui-testing` skill and follow `.claude/rules/tests.md` when writing them.

- `?` inside `cx.spawn` closures needs the closure to return `Result`; end the task with `.detach_and_log_err(cx)` *and* surface the error to the user (AGENTS.md › Errors must reach the user).
- `WeakEntity::update` returns `Result` — the entity may be gone after an await. Handle it; don't unwrap.
- Holding a `RefCell`/`Mutex` borrow across `.await` deadlocks or panics. Scope the borrow before awaiting.
- `impl Drop` that touches GPUI entities runs at unpredictable times — clean up via subscriptions/`on_release` instead.
- `HashMap` iteration order is random: anything rendered or serialized from a map must be sorted or use `IndexMap`/`BTreeMap`, or UI order and saved files jitter.
- Converting `usize` ↔ `u32`/`f32` for layout: use `try_from`/explicit clamps, not `as`, when the value comes from user data (response sizes, row counts).
