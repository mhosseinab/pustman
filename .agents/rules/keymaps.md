---
paths:
  - "assets/keymaps/*.json"
---

# Keymap traps

See the `gpui-actions-keymaps` skill.

- Change all three files (`default-macos.json`, `default-linux.json`, `default-windows.json`) together; macOS uses `cmd-`, the others `ctrl-`.
- Grep all three for the keystroke before adding it — a later entry with the same key and an equal-or-deeper context silently wins.
- The `context` string must match a `.key_context("…")` that actually exists in code, or the binding never fires.
- Action names are `namespace::Name` exactly as declared; a typo surfaces only as a runtime notification.
