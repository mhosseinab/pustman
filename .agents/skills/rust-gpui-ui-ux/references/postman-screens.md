# Pustman screens — layout and behavior recipes

The window follows Zed's workspace anatomy, mapped onto Postman's mental model:

```
┌ TitleBar ─ [workspace switcher] ───────── [env picker ▾] [⚙] ┐
├────────────┬─────────────────────────────────────────────────┤
│ Left Dock  │ Center: PaneGroup                                │
│ ┌────────┐ │ ┌ TabBar: [GET Users ●] [POST Login] [+]        ┐│
│ │Collect.│ │ │ RequestEditor (Item)                          ││
│ │History │ │ │  URL bar                                      ││
│ │Envs    │ │ │  [Params|Auth|Headers(3)|Body|Scripts|Settings]││
│ └────────┘ │ │  ───────── resizable split ─────────          ││
│  tree      │ │  ResponseViewer                               ││
│            │ │  200 OK · 142 ms · 3.2 KB   [Body|Headers|..] ││
│            │ └───────────────────────────────────────────────┘│
├────────────┴─────────────────────────────────────────────────┤
│ StatusBar: [console] [cookies] … [proxy: off] [online ●]      │
└──────────────────────────────────────────────────────────────┘
```

- Docks hold `Panel`s (collections, history, environments, console). Panels persist size/position/visibility.
- Center holds `Item`s (request, collection overview, environment editor). Splits are supported; a request tab
  can be split to compare responses.
- Modals (`ModalLayer`, one at a time): import, command palette, environment picker, go-to-request.
- Toasts (`ToastLayer`): transient confirmations; errors that need action use a notification with buttons.

## Collections panel (left dock)

- Tree of Workspace → Collection → Folder → Request. Virtualized with `uniform_list`; flatten the visible
  tree into a `Vec<VisibleEntry { depth, kind, id, is_expanded }>` recomputed only on structural change.
- Row: indent (`indent_level`), disclosure, method badge (fixed-width column, monospace XSmall, method color),
  name (truncate, tooltip on overflow), end slot revealed on hover (`…` menu).
- Keyboard: ↑/↓ select, ←/→ collapse/expand or move to parent/child, Enter opens (preview tab), cmd/ctrl-Enter opens pinned,
  F2 rename inline, Delete with confirmation, `/` or cmd-F filter. Mirror Zed's project panel actions.
- Mouse: single click selects + opens preview tab (italic title until edited), double click pins, drag to reorder/move
  (`on_drag`/`drag_over`/`on_drop` with a drop-indicator line, not just a background), right-click → `right_click_menu`.
- Empty states: "No collections yet" + [New Collection] [Import…] buttons + keybinding hints.
- Filter: fuzzy match highlights with `HighlightedLabel`; keeps ancestors of matches visible.

## Request tabs (center tab bar)

- Tab content: method badge + name; dirty `Indicator` dot replaces the close button until hover; italic for preview tabs.
- `Item` impl: `tab_content`, `tab_tooltip_text` (full URL), `is_dirty`, `can_save`, `save`, `navigate` for back/forward.
- Closing a dirty tab prompts Save / Don't Save / Cancel (`on_before_dismiss`-style decision); never silently discard.
- Overflow: tab bar scrolls horizontally; active tab always scrolled into view; cmd-1…9 activate.

## URL bar

```
[GET ▾] [ https://{{base_url}}/users?page=2                         ] [Send ▾]
```
- Single-line `text_input` with monospace `text_buffer`; `{{variables}}` highlighted resolved/unresolved;
  hover a variable → popover with value, scope, and "Edit" action. Never show secret values in the hover.
- Paste a cURL command → offer to import it (inline `Callout`, not an automatic rewrite).
- Editing the query string updates the Params table and vice versa (one source of truth: the parsed URL model).
- Send: `Button` Filled/accent, `.loading(true)` while in flight and becomes **Cancel** (Escape also cancels).
  cmd/ctrl-Enter sends from anywhere inside the editor's key context. Split button `▾`: Send and Download.
- Invalid URL: inline error under the bar, Send disabled with tooltip explaining why.

## Key/value tables (Params, Headers, Form-data, x-www-form-urlencoded)

- Columns: [enabled checkbox] Key | Value | Description | [row actions]. Resizable, keyboard navigable cells.
- Always one trailing empty "ghost" row; typing in it creates a real row.
- Tab moves cell → cell → next row; Enter in last cell adds row; cmd-Backspace deletes row; Space toggles enabled.
- Disabled rows render muted with strikethrough-free reduced opacity; duplicates flagged with a warning indicator.
- Auto-generated headers (Content-Length, User-Agent) shown in a collapsible "Auto-generated" section, read-only with a lock icon.
- Bulk edit toggle switches to a text area of `key: value` lines (round-trips losslessly).
- Count in the sub-tab label: `Headers (3)`.

## Body editor

- Mode dropdown: none | form-data | x-www-form-urlencoded | raw (JSON/XML/Text/HTML/JS) | binary | GraphQL.
- Raw: multi-line `text_input` with line numbers, monospace, soft wrap toggle, JSON format action (`cmd-shift-F`),
  inline parse error with line/column. Large bodies edited off the main thread for formatting.
- Binary: file picker via platform dialog (`cx.prompt_for_paths`), shows name + size, never loads the file into the UI.

## Response viewer

- Status row: `StatusPill` (code + reason, colored by class), time, size, each with a hover breakdown
  (DNS, connect, TLS, TTFB, download; headers vs body bytes). Tabular numbers.
- Tabs: Body | Cookies | Headers (n) | Timeline. Body sub-modes: Pretty | Raw | Preview (HTML/image) | Hex for binary.
- Body is read-only, virtualized by line (`uniform_list`), with find (cmd-F), copy, save-to-file, wrap toggle.
  Pretty-printing and syntax tokenization happen in `background_spawn`; show Raw immediately and swap when ready.
- Bodies over a threshold (setting, default 10 MB) are not rendered inline: offer "Open in external editor" / "Save".
- States: never-sent (empty state with send hint), sending (spinner + elapsed time, Cancel), success, HTTP error (still a
  success at transport level — show the body!), transport error (DNS/TLS/timeout: `Callout` with the cause + suggested fix + Retry),
  cancelled.
- The request/response split is a resizable divider; layout switches between vertical and side-by-side (setting + action).

## Environment picker

- `PickerDelegate` in a modal (`ModalView`), fuzzy matched, opened from title bar chip or `environment::Select`.
- Rows: name, variable count, "active" check. Footer actions: Manage Environments, No Environment.
- Title bar chip shows active env name with a color dot; "No Environment" is muted, not hidden.

## Command palette

- Modal `Picker` over all registered actions available in the current focus context; shows keybinding per row (`KeyBinding`).
- Recently used actions first; humanized action names (`request_editor::SendRequest` → "Request Editor: Send Request").

## Console / history panels

- Console (bottom dock): variable-height `list` with `ListAlignment::Bottom` and follow-tail; entries expandable to raw
  request/response. Redact secrets.
- History: grouped by day with `ListHeader`s; click restores the request into a new preview tab.

## Settings UI

- Settings are JSON-first (`settings.json`, schema-validated with completions) with a settings view for common options.
  Every setting change applies live (observe `SettingsStore`), no restart.
