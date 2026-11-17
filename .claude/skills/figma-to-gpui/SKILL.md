---
name: figma-to-gpui
description: Implement Pustman UI components and screens from Figma designs via the Figma MCP server — read design context, screenshot, variables and assets, then translate the React/Tailwind reference output into GPUI Rust that reuses our ui crate components and theme tokens.
when_to_use: Use when the user gives a figma.com design URL or node and wants a GPUI component, view or screen built or updated to match it, or asks to check an implemented component against its Figma design.
---

# Figma → GPUI

The Figma MCP server has no Rust/GPUI output. `get_design_context` returns **React + Tailwind reference code**, a
screenshot, and asset URLs. GPUI's styling API is Tailwind-shaped, so the reference translates almost 1:1 — but it
is a *visual prototype*, not code to paste. Our `ui` components, theme tokens and interaction rules win over it.

Load these first: Figma's `figma-design-to-code` skill (mandatory before `get_design_context`; installed plugin skill,
or MCP resource `skill://figma/figma-design-to-code/SKILL.md`), then `rust-gpui-ui-ux`.

## Workflow

1. **Parse the URL.** `figma.com/design/:fileKey/:name?node-id=1-2` → `fileKey`, `nodeId = "1:2"`
   (branch URLs: use the branch key). No `node-id` → ask the user for a node link; never guess.
2. **Open a session.** `create_session(fileKey, task)` and pass its `sessionId` to every later file-tool call.
3. **Read the design.** `get_design_context(fileKey, nodeId, clientLanguages: "rust", clientFrameworks: "gpui", skillNames: "figma-design-to-code")`
   (if the skill was loaded as an MCP resource rather than a plugin skill, pass `"resource:figma-design-to-code"`)
   with the screenshot included. If the response is flagged sparse, fetch the visible child nodes in one parallel batch.
   Also call `get_variable_defs` on the same node to see which Figma variables it uses.
4. **Inventory before writing.** For every element in the reference:
   - existing `ui` component? (`Button`, `IconButton`, `Tab`, `ListItem`, `Label`, `Icon`… — see
     `rust-gpui-ui-ux/references/components.md`) → reuse/configure it, add a variant only if no config expresses the design;
   - Figma variable? → the matching theme token / spacing helper (`figma-tokens-to-theme` keeps them in sync);
     a raw hex or px with **no** variable behind it → ask whether it should become a token before hard-coding;
   - asset? → step 6.
5. **Translate** the layout with [references/tailwind-to-gpui.md](references/tailwind-to-gpui.md). Replace absolute
   positioning with flex layout unless the design is truly overlaid. Decide what is interactive (Figma never says) and add
   ids, hover/active/focus states, `on_click` → action, `key_context`/`track_focus`, `tab_index`, accessible labels.
   Implement the states the frame doesn't show (empty, loading, error, disabled, overflow/truncation, narrow width).
6. **Assets.** Download every asset as the `get_design_context` response instructs; no Figma URLs may remain in code.
   - single-color icons → `assets/icons/<snake_case>.svg`, rendered with `svg().path(...)` **plus `.text_color(...)`**
     (GPUI paints SVGs as a one-color mask in the text color; without a text color nothing is drawn). If the icon
     already exists in `assets/icons/` (Lucide-derived), reuse it instead of adding a duplicate.
   - multi-color illustrations/logos/raster → `assets/images/`, rendered with `img(...)`; keep the SVG's own size.
   - never redraw, trace, or generate replacement art (see the user's image rules); if an asset is missing, ask.
7. **Test** (`gpui-testing`): behavior (clicks/keys dispatch the right actions, states switch), not pixels.
8. **Verify visually.** Run the app or the component gallery and compare against the Figma screenshot at the same
   size, in light and dark. Check spacing, type size/weight, colors, radii, borders, icon size/position, truncation.
   If you cannot run the app (e.g. missing system deps), say the visual check was not done — don't claim a match.
   Note out-of-scope mismatches; don't fix them.

## Code Connect

Code Connect has no Rust/GPUI label, so Figma can't natively point a component at `crates/ui/src/components/button.rs`.
Don't build tooling around it. When a Figma component maps to a `ui` component, record it in the component's doc comment
(`/// Figma: <component name> (<fileKey>:<nodeId>)`) so later translations reuse it.

## Traps
- Copying Tailwind arbitrary values (`px-[13px]`, `#3A3A3A`) straight into Rust → off-system values; map to tokens or ask.
- Fixed `w(px(…))`/`h(px(…))` from the frame size → breaks resizing; use `flex_1`, `w_full`, `min_w_0`, content sizing.
- Figma text with no truncation rule → long URLs/header values overflow; decide truncate vs wrap per element.
- Hover/pressed frames in Figma are variants of one component, not separate components.
- Figma auto-layout `gap`/`padding` in px vs our density-aware spacing: use the `ui` spacing helper whose default matches.
