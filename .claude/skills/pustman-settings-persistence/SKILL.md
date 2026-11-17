---
name: pustman-settings-persistence
description: Add typed, live-reloading user settings (JSON + defaults + schema) and persist app/UI state (SQLite + key-value store) in Pustman following Zed's settings and db patterns, reimplemented without GPL code.
when_to_use: Use when adding a setting, reading settings in a view, reacting to settings changes, persisting open tabs/dock sizes/history/collections, writing a DB migration, or storing secrets.
---

# Settings & persistence

Zed's `settings`, `db` and `sqlez` crates are GPL-3.0 — we reproduce the *design*, not the code.

## Settings design

- **Files:** `assets/settings/default.json` (bundled, commented, complete) → user `<platform config dir>/pustman/settings.json` (Linux `~/.config`, macOS `~/Library/Application Support`, Windows `%APPDATA%`)
  → workspace-local `.pustman/settings.json`. Later layers override earlier ones key by key.
- **Content structs** mirror the JSON with every field `Option<T>` (so partial user files merge), derive
  `Serialize, Deserialize, JsonSchema`. The schema powers validation and editor completions.
- **Typed settings** are the resolved, non-optional view used by code:

```rust
#[derive(Clone, Debug)]
pub struct HttpSettings {
    pub timeout: Duration,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub verify_tls: bool,
    pub max_inline_response_bytes: u64,
}

impl Settings for HttpSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        let http = content.http.as_ref();   // defaults guarantee presence after merge
        Self { … }
    }
}

// read
let settings = HttpSettings::get_global(cx);

// react (store the subscription)
self._subscriptions.push(cx.observe_global::<SettingsStore>(|this, cx| {
    this.apply_settings(HttpSettings::get_global(cx));
    cx.notify();
}));
```

- `SettingsStore` is a global; it watches the files (background task), re-parses on change, and on parse error keeps
  the last good settings and shows a notification with the file + line. Never panic on bad user JSON.
- Every setting: default in `default.json` with a comment, schema doc string, applies live, documented.
- UI settings to provide from day one: `theme` (light/dark/system), `ui_font_size`, `buffer_font_family/size`,
  `ui_density`, `response.layout` (vertical/side-by-side), `response.wrap`, `http.*`, `proxy.*`, `telemetry` (off by default).

## Persistence

| Data | Store | Notes |
| --- | --- | --- |
| Collections, environments | Files (Postman v2.1-compatible JSON) under the workspace dir | Users version them in git; write atomically (temp + rename) |
| History, open tabs, dock sizes, last selected env, UI state | SQLite (`crates/storage`) | Per-workspace rows keyed by workspace id |
| Small flags (dismissed banners, last version run) | Key-value table | `kv::read(key)`, `kv::write(key, value)` |
| Secrets (tokens, passwords, secret variables) | OS keychain (e.g. `keyring` crate) | Only a reference id in files/DB |

SQLite pattern (Zed's `Domain` idea): each crate that persists declares its domain with a name and an ordered list of
migration SQL strings; the store applies pending migrations at startup. Migrations are append-only — never edit a
shipped migration. All queries run off the main thread (`cx.background_spawn`) and return `anyhow::Result`.

```rust
pub struct HistoryDb(ThreadSafeConnection);
impl Domain for HistoryDb {
    const NAME: &str = "history";
    const MIGRATIONS: &[&str] = &[
        "CREATE TABLE history(id INTEGER PRIMARY KEY, workspace_id INTEGER NOT NULL, method TEXT NOT NULL,
          url TEXT NOT NULL, status INTEGER, duration_ms INTEGER, sent_at TEXT NOT NULL) STRICT;",
    ];
}
```

(Choose the SQLite binding — `rusqlite` or `sqlx` — once, in `crates/storage`; features don't touch SQL drivers.)

## Traps
- Reading files/DB in `render` or a foreground handler → use background tasks and cache results in the view.
- Serializing UI state on every frame → debounce with a GPUI timer (e.g. 500 ms) and on window close.
- Forgetting migration on schema change → app fails for existing users; add a migration test that upgrades from the previous schema.
- Persisting secrets in history → redact `Authorization`, cookies, and secret variables before storing.
