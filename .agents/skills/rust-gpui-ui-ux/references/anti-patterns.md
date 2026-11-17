# UI anti-patterns (review checklist)

Zed enforces several of these with custom dylint lints (`tooling/lints`); names given in parentheses.

| Anti-pattern | Why it hurts | Do instead |
| --- | --- | --- |
| `cx.notify()` inside `render` (`notify_in_render`) | Infinite re-render loop | Notify in the handler that mutated state |
| `entity.update(...)` inside `render` (`entity_update_in_render`) | Mutation during paint, re-entrancy panics | Compute in handlers/observers; render reads only |
| `SharedString::from("literal")` / `"lit".to_string().into()` (`shared_string_from_str_literal`, `owned_string_into_shared`) | Needless allocation per frame | `SharedString::from_static("literal")`; pass `&'static str` where accepted |
| Blocking IO in a foreground handler or `cx.spawn` (`blocking_io_on_foreground`) | Frozen UI | `cx.background_spawn` / `Tokio::spawn` |
| `async move {}` with no `.await` (`async_block_without_await`) | Pointless task, hides sync work | Just call the function |
| `if !map.contains_key(k) { map.insert(..) }` (`map_lookup_then_insert`) | Double lookup | `entry` API |
| `let _task = cx.spawn(...)` / dropping a `Task` | Silently cancelled | Store, await, or `detach_and_log_err` |
| Dropping a `Subscription` | Handler never fires | Keep in `_subscriptions` |
| Using the outer `cx` inside `update(cx, \|this, cx\| …)` | Borrow errors or stale context | Use the inner `cx` |
| Hard-coded colors `rgb(0x…)`, `hsla(…)` in feature code | Breaks themes, contrast | Theme token or `Color` |
| Pixel sizes for text/spacing (`px(13.)`) | Ignores UI scale & density | `rems`, `TextSize`, `DynamicSpacing` |
| Unvirtualized `.children(items.iter().map(...))` for big lists | Frame time grows with data | `uniform_list` / `list` |
| Row IDs from enumerate index on reorderable lists | Focus/hover jumps to wrong row | IDs from stable model IDs |
| Formatting/parsing in `render` | Frame drops on big responses | Precompute on change, background task |
| Icon button without tooltip/label | Undiscoverable, inaccessible | `.tooltip(Tooltip::text(..))` + label |
| Error only in `log::error!` | User thinks it worked | Toast/notification/inline Callout |
| Modal that steals focus and doesn't return it | Keyboard users get lost | Restore previous focus on dismiss |
| Spinner without cancel on network work | User stuck | Cancel action + Escape |
| Ad-hoc component styling repeated in features | Inconsistent UI | Add/extend a component in `crates/ui` |
| Copying Zed GPL code | License contamination | Reimplement the pattern |
| `smol::Timer`/`thread::sleep` in tests | Nondeterministic tests | `cx.background_executor().timer(d)` |
