# UI performance

Budget: **≤ 8 ms per frame** (120 Hz displays). Input → visible feedback within one frame.

## Render path
- `render` must be O(visible items). No sorting, filtering, parsing, formatting of large data, or
  allocation proportional to data size inside `render`. Precompute on change, cache in the view.
- Virtualize anything that can exceed ~50 rows (`uniform_list` for fixed height, `list` + `ListState` for variable).
- Prefer `SharedString` (cheap clone, `Arc<str>`) for text you render repeatedly; `SharedString::from_static` for literals.
- Pass `Arc<T>` to children rather than cloning large structs (response bodies, header lists).
- Don't `cx.notify()` in a loop; batch mutations then notify once. Don't notify when nothing changed.
- Keep entity count proportional to *visible* complexity: a 10k-row table is one view with a virtualized list, not 10k entities.

## Off the main thread
| Work | Where |
| --- | --- |
| Network (reqwest) | `gpui_tokio::Tokio::spawn` inside `http_engine` |
| JSON pretty-print, syntax tokenization, search in body | `cx.background_spawn` |
| File/DB IO (history, collections, settings) | `cx.background_spawn` (never blocking IO on foreground) |
| Collection import/export | `background_spawn`, progress notification, cancellable `Task` |

Results come back via `this.update(cx, …)` and a single `cx.notify()`.

## Large responses
- Stream the body to a temp file + bounded in-memory prefix; render from line index, not one giant string.
- Show Raw first (cheap), swap in Pretty when the background formatter finishes.
- Hard cap for inline rendering (setting); beyond it offer Save/Open externally.
- Avoid shaping very long single lines: soft-wrap or truncate at N chars with "show full line".

## Measuring
- Zed's `gpui` has a profiler and inspector (`inspector.rs`, `profiler.rs`); enable the inspector in debug builds
  to check element bounds and styles.
- Add a benchmark/test that renders the worst case (10k-row tree, 5 MB JSON) and asserts no unbounded work.
- In tests, `cx.run_until_parked()` then assert; a hang usually means a task that never completes.
