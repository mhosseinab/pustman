---
name: rust-gpui-ui-ux
description: Build Pustman's native Rust UI with GPUI using Zed's architecture as the reference. Covers views vs components, entities/context, Tailwind-style styling, the design system (theme tokens, semantic Color, typography, density-aware spacing, elevation), component builders, Postman screen recipes (collections tree, request tabs, URL bar, key/value tables, body editor, response viewer, environment picker, command palette), virtualized lists, focus, keyboard-first interaction, accessibility, theming, performance and the UX quality checklist. Use whenever writing or reviewing any Rust code that renders UI — impl Render/RenderOnce, div()/h_flex()/v_flex(), styling, layout, panels, modals, popovers, menus, tooltips, tables, lists, text inputs, theme colors, icons, empty/loading/error states, keyboard navigation, or UX polish in Pustman.
---

# Rust GPUI UI/UX for Pustman

GPUI is a GPU-rendered, immediate-mode-*rendering*, retained-*state* UI framework. Views (`Entity<T>`
that `impl Render`) own state; every frame they rebuild a cheap element tree styled with Tailwind-like
methods. Zed (commit `58849026`) is our reference app. Copy its *shapes*, write our own code.

## Mental model in 8 lines

1. **State lives in entities.** `cx.new(|cx| View::new(cx))` → `Entity<View>`. Hold `WeakEntity` for back-references.
2. **Render is pure.** `fn render(&mut self, window, cx) -> impl IntoElement` reads state, returns elements. No `cx.notify()`, no `entity.update` inside.
3. **Mutate → notify.** Event handlers mutate `self` then call `cx.notify()`.
4. **Components are values.** Stateless pieces are `#[derive(IntoElement)] struct X` + `impl RenderOnce` with a builder API.
5. **Communication** is `cx.emit(Event)` + `cx.subscribe(...)`, or `cx.observe(...)` on `notify`. Store the `Subscription`.
6. **Input is actions.** Keybindings dispatch typed actions to the focused element's `key_context`; handlers are `.on_action(cx.listener(Self::method))`.
7. **Async is tasks.** `cx.spawn(async move |this, cx| …)`; dropping the `Task` cancels it.
8. **Colors come from the theme, never literals.** `cx.theme().colors().border`, or semantic `Color::Muted` on labels/icons.

## Workflow for any UI change

1. Find the Zed analogue (table in [references/zed-map.md](references/zed-map.md)) and read its shape.
2. Decide **view or component** ([references/gpui-fundamentals.md](references/gpui-fundamentals.md#views-vs-components)).
3. Build from design-system primitives only ([references/design-system.md](references/design-system.md),
   [references/components.md](references/components.md)). Add a missing primitive to `crates/ui` instead of inlining one-off styling.
4. Wire actions, a `key_context`, focus, and keybindings for every command ([references/interaction-focus-a11y.md](references/interaction-focus-a11y.md)).
5. Design all states: empty, loading, partial, error, offline, overflowing content, narrow pane.
6. Use the Postman screen recipe if one applies ([references/postman-screens.md](references/postman-screens.md)).
7. Check performance rules ([references/performance.md](references/performance.md)).
8. Write a GPUI test (`gpui-testing` skill) and walk the [UX checklist](references/ux-checklist.md).
9. Scan [references/anti-patterns.md](references/anti-patterns.md) before finishing.

## Design source: `docs/postman_redesign_pack/`

The redesign pack is the **design reference** for every Pustman screen. Before you build or review a screen,
read the files that apply to it:

| Need | Read |
| --- | --- |
| What a screen must show and do in each state | `01_specs/screen_specs.md` (S01–S24) and its rows in `01_specs/screen_state_matrix.csv` |
| Colours, spacing, radii, row heights, type scale, component inventory | `01_specs/design_system.md`, `01_specs/design_tokens.json` |
| Layout direction | `06_visuals/workbench_blueprint.svg` (the blueprints are proposals, not specs) |
| Realistic content: long strings, errors, empty data | `04_fixtures/` |
| Review gates for a finished screen | `05_validation/design_review_checklist.md`, `05_validation/token_contrast.csv` |

Precedence when sources disagree:
1. **`docs/PRD.md` decides scope.** The pack covers more than v1 does. Cloud collaboration screens (members,
   invitations, sharing, comments, cloud review, notifications: S16–S23) are out of scope, because sharing is
   Git-based (PRD §6). Build them only when the PRD calls for them.
2. **The pack decides how in-scope screens look and behave.** Its token *values*, states and layout win over
   anything in `references/postman-screens.md`. Our token *names* and the structure in
   `references/design-system.md` stay. Map pack values onto them; don't rename our tokens to match the pack.
3. **This skill's GPUI rules and the non-negotiables below always apply.** The pack was written for Figma, so its
   Figma instructions (pages, frames, Code Layers, Make) are not steps for us.

Screenshots in `03_references/screenshots/` and the PDFs in `07_guides/` show Postman's own UI. Use them to
understand behaviour only. Never copy their artwork, icons, branding or layout pixel for pixel into `assets/` or code.
When the pack and our code disagree and the pack is right, fix the code. When it is ambiguous, record your
decision in the step's `Decision:` lines.

## Non-negotiables

- Keyboard-first: every action reachable without a mouse, discoverable through tooltip + menu + command palette.
- Light **and** dark theme correct; no hard-coded `rgb(...)` outside theme definitions.
- Sizes in `rems` (scale with `ui_font_size`), spacing via `DynamicSpacing` (scales with density).
- Text that can overflow truncates (`.truncate()`/`.text_ellipsis()`) with the full value in a tooltip, and containers that shrink have `.min_w_0()`.
- Long collections (tree rows, history, headers, response lines) are virtualized (`uniform_list`/`list`).
- Feedback within one frame; anything >100ms shows progress; a frame budget is **≤8ms**.
- Errors are actionable ("Could not resolve host `api.example.com` — check the URL or your network"), shown where the user is looking.
- Every interactive element: stable `.id()`, accessible role/label, visible focus state.

## Reference files

| File | Read when |
| --- | --- |
| [gpui-fundamentals.md](references/gpui-fundamentals.md) | Entities, contexts, render, events, tasks, elements, styling API, lists, drag/drop, overlays |
| [design-system.md](references/design-system.md) | Theme tokens, semantic colors, typography, spacing, density, elevation, icons |
| [components.md](references/components.md) | Writing/using components: Button, IconButton, Label, Tab, ListItem, Table, Modal, Popover, ContextMenu, Tooltip, Toast, Callout |
| [postman-screens.md](references/postman-screens.md) | Layout + behavior for each Pustman screen |
| [interaction-focus-a11y.md](references/interaction-focus-a11y.md) | Actions, key contexts, focus, tab order, AccessKit roles |
| [performance.md](references/performance.md) | Frame budget, virtualization, background work, large responses |
| [ux-checklist.md](references/ux-checklist.md) | Before calling UI work done |
| [anti-patterns.md](references/anti-patterns.md) | Review checklist of known traps |
| [zed-map.md](references/zed-map.md) | Where in Zed to look for each pattern |
