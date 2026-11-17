---
paths:
  - "assets/themes/*.json"
  - "assets/settings/*.json"
---

# Theme & settings JSON traps

- A new theme token must be added to **every** bundled theme (light and dark), with contrast ≥ 4.5:1 for text tokens against their background.
- Colors are `#rrggbbaa` hex strings; don't mix formats.
- `assets/settings/default.json` is the documentation users read: every key has a comment and the real default. A key in the typed settings without a default here is a bug (see `pustman-settings-persistence`).
- Renaming a settings key breaks user files — keep reading the old key and migrate it.
