---
paths:
  - "crates/ui/**/*.rs"
  - "crates/workspace/**/*.rs"
  - "crates/*_panel/**/*.rs"
  - "crates/request_editor/**/*.rs"
  - "crates/response_viewer/**/*.rs"
  - "crates/*_picker/**/*.rs"
  - "crates/command_palette/**/*.rs"
  - "crates/text_input/**/*.rs"
---

# UI render traps

Load the `rust-gpui-ui-ux` skill before editing these files.

- No raw `rgb(…)`/`hsla(…)` literals in views — use `cx.theme().colors()` tokens; add a token if one is missing.
- No raw `px(…)` spacing in feature crates — use the `ui` spacing/density helpers.
- Flex children with text need `min_w_0()` (or `overflow_hidden()`), otherwise `truncate()` does nothing and the row overflows.
- Lists over user data (collections, history, headers, response lines) use `uniform_list`/`list`; never `.children(items.iter().map(…))` for unbounded data.
- Element IDs come from stable model IDs (`request.id`), not loop indices — index IDs break hover/focus/scroll state on reorder.
- Clickable `div`s need `.id(…)`, `.cursor_pointer()`, a keyboard path (`on_action`/`track_focus`), and an accessible label.
- Heavy work (JSON pretty-print, syntax spans, search) is computed when data changes and cached in the view, never in `render`.
