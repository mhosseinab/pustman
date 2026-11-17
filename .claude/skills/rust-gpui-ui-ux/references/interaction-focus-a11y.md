# Interaction, focus and accessibility

Detailed action/keymap mechanics live in the `gpui-actions-keymaps` skill. This file covers how they
shape UI design.

## Keyboard-first design

1. Every command is an **action** first, then bound to buttons/menus/keys. A button's `on_click` should
   dispatch the same action (`window.dispatch_action(SendRequest.boxed_clone(), cx)`) or call the same handler.
2. Each focusable view sets `.key_context("RequestEditor")` and `.track_focus(&self.focus_handle)`; handlers
   are registered on that element with `.on_action(cx.listener(Self::send))`.
3. Discoverability: tooltip shows the binding (`Tooltip::for_action`), context menus show bindings, command palette lists everything, empty states teach 1–3 key shortcuts.
4. Escape always means "back out one level": close popover → cancel inline edit → cancel in-flight request → dismiss modal → focus parent pane.

## Focus

```rust
pub struct CollectionsPanel { focus_handle: FocusHandle, … }

impl CollectionsPanel {
    fn new(cx: &mut Context<Self>) -> Self {
        Self { focus_handle: cx.focus_handle(), … }
    }
}

impl Focusable for CollectionsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}
```

- A modal/picker's `Focusable` returns its *input's* handle so typing works immediately.
- Move focus explicitly after structural changes: opening a request focuses the URL bar if empty, otherwise the tab; closing a modal returns focus to where it came from.
- Query focus with `focus_handle.is_focused(window)` / `contains_focused(window, cx)` to style the focused pane border (`pane_focused_border`).
- Visible focus: `.focus_visible(|style| style.border_color(cx.theme().colors().border_focused))` — shows only for keyboard focus, like CSS `:focus-visible`.
- Tab order: `cx.focus_handle().tab_index(n).tab_stop(true)`, `.tab_group()` on containers; `window.focus_next(cx)` / `focus_prev(cx)` for Tab/Shift-Tab. Order must follow visual reading order: method → URL → Send → sub-tabs → table.

## Pointer interaction standards

- Hit targets ≥ 22px high for buttons in dense rows (`ButtonSize::Default`), 28px in toolbars.
- Hover reveals secondary actions (`visible_on_hover(group)`) but they must still be keyboard reachable via the row's context menu.
- Right-click anywhere meaningful opens a context menu consistent with the keyboard actions.
- Drag targets show an insertion indicator; drop is undoable.
- Double-click semantics consistent across tree and tabs (pin/open).

## Accessibility (AccessKit)

GPUI exposes an accessibility tree via AccessKit (`gpui::Role`, re-exported from `accesskit`).

```rust
div()
    .id("send-button")
    .role(Role::Button)
    .aria_label("Send request")
    .aria_keyshortcuts("Control+Enter")
```

Available on stateful elements: `role`, `accessibility_id`, `aria_label`, `aria_description`,
`aria_keyshortcuts`, `aria_selected`, `aria_expanded`, `aria_toggled`, `aria_level`, `aria_value`,
`aria_numeric_value` (+ min/max/step), `aria_active_descendant`.

Rules:
- Components set roles internally (Button → `Role::Button`, ListItem → list item / tree item, Tab → `Role::Tab`, Switch → `Role::Switch`).
- Icon-only controls must have `aria_label`. Labels describe the action, not the icon ("Close tab GET Users", not "X").
- Tree: `aria_level(depth)`, `aria_expanded(open)`, `aria_selected(selected)`.
- Live regions for async results: announce "Response 200 OK in 142 milliseconds" on completion.
- `Role::GenericContainer` is filtered out — don't set it.
- Element IDs must be globally unique per frame; use `(name, stable_id)` tuples.
- `accessibility_id` is visible outside the process — keep it stable (useful for UI automation tests).
- Reference: `crates/gpui/examples/a11y.rs` and `crates/gpui/src/_accessibility.rs` in Zed.

## Text input

GPUI has no built-in text field. Zed's input wraps its GPL `editor` crate. Pustman builds `crates/text_input` from
the Apache-licensed pattern in `crates/gpui/examples/input.rs`:

- A view holding `content`, `selected_range`, `selection_reversed`, `marked_range` (IME composition), cached `last_layout`/`last_bounds`.
- Implements `EntityInputHandler` (text_for_range, selected_text_range, marked_text_range, replace_text_in_range,
  replace_and_mark_text_in_range, bounds_for_range, character_index_for_point) so IME, emoji picker and accessibility work.
- A custom `Element` (`TextElement`) that shapes the line in `prepaint` and in `paint` calls
  `window.handle_input(&focus_handle, ElementInputHandler::new(bounds, entity), cx)`, then paints selection and cursor.
- Actions: Backspace, Delete, Left/Right, SelectLeft/Right, SelectAll, Home/End, Copy/Cut/Paste, ShowCharacterPalette.
- Multi-line extends it with line layout per visible line, vertical movement, and virtualization for bodies.

Treat `text_input` as core infrastructure with its own heavy test suite (IME, grapheme clusters, selection).
