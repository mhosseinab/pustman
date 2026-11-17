---
name: figma-tokens-to-theme
description: Sync Figma variables (colors, spacing, radii, typography) into Pustman's theme JSON files and ui spacing/typography scales via the Figma MCP get_variable_defs tool, mapping Figma names to our token names for light and dark themes. Use when the user wants Pustman's theme or design tokens to match a Figma file, when a Figma design uses variables with no matching theme token, or when the design system's colors/spacing change in Figma.
---

# Figma variables → theme tokens

When no Figma file is available, use `docs/postman_redesign_pack/01_specs/design_tokens.json` (Light and Dark
modes) as the source of values, mapped through the same steps below. Its `05_validation/token_contrast.csv` has the
expected contrast results to check against.

Our tokens live in `assets/themes/*.json` (colors per theme) and the `ui` crate (spacing, radii, type scale) — see
`rust-gpui-ui-ux/references/design-system.md`. Figma is a source of *values*; our token *names* and structure stay ours.

## Workflow

1. Get a node link that uses the variables (ideally a token/style-guide frame). `create_session`, then
   `get_variable_defs(fileKey, nodeId)` → `{ "color/bg/surface": "#1E1E1E", "space/2": 8, … }`.
2. **Modes.** `get_variable_defs` reports the values for the node it is given. To get light *and* dark, ask for one frame per
   mode (the same frame duplicated with the dark mode applied) and call it on each. Never derive dark values yourself.
3. **Map names.** Build a table Figma variable → Pustman token (`color/bg/surface` → `surface_background`). Show it to the user
   before writing when any row is a guess, a new token, or a rename. Unmapped Figma variables are listed, not silently added.
4. **Write.**
   - colors → every bundled theme file, `#rrggbbaa`; a new token is added to all themes and to `ThemeColors`
     (with a `figma: <variable name>` doc comment).
   - spacing/radii/type → the `ui` scales only if Figma's values differ from the scale; don't add one-off steps.
5. **Check.** Text/background token pairs keep ≥ 4.5:1 contrast (3:1 for large text and UI glyphs); report any pair that
   fails rather than nudging Figma's values. Run the theme crate's tests (deserialize every bundled theme).

## Traps
- Figma variable *aliases* (`button/bg` → `color/accent`) should map to our semantic token, not be flattened into a second raw color.
- A Figma color with alpha must keep it (`#rrggbbaa`); converting to opaque changes how it blends over surfaces.
- Spacing in Figma is px at 1×; our spacing is density-aware rems — map to the scale step whose default equals the px value.
