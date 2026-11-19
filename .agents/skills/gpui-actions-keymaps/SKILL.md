---
name: gpui-actions-keymaps
description: Define GPUI actions, register handlers, bind keys via per-platform JSON keymaps with key contexts, and avoid binding conflicts in Pustman. Use when adding a command, shortcut, menu entry, command-palette item, key_context, on_action handler, or editing assets/keymaps/*.json.
---

# Actions & keymaps

Verified against Zed `0cc1ffb6`. Every user-facing command is an action. Buttons, menus, keybindings
and the command palette all dispatch the same action.

## 1. Declare

```rust
// Unit actions: doc comments become command-palette/help text.
actions!(
    request_editor,
    [
        /// Sends the current request.
        SendRequest,
        /// Cancels the in-flight request.
        CancelRequest,
        /// Pretty-prints the request body.
        FormatBody,
    ]
);

// Actions with data: deserialized from keymap JSON.
/// Activates the request tab at the given index.
#[derive(Clone, PartialEq, Debug, Deserialize, JsonSchema, Default, Action)]
#[action(namespace = workspace)]
#[serde(deny_unknown_fields)]
pub struct ActivateTab(pub usize);
```

- Namespace = crate's feature name (`request_editor::SendRequest`). Names are verbs.
- Renaming an action breaks user keymaps: keep the old name with `#[action(deprecated_aliases = ["old::Name"])]`.

## 2. Handle

```rust
impl Render for RequestEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("RequestEditor")            // context name = view type name
            .track_focus(&self.focus_handle)         // actions dispatch along the focus path
            .on_action(cx.listener(Self::send_request))
            .on_action(cx.listener(Self::cancel_request))
            .on_action(cx.listener(Self::format_body))
            …
    }
}

impl RequestEditor {
    fn send_request(&mut self, _: &SendRequest, window: &mut Window, cx: &mut Context<Self>) { … }
}
```

- Dispatch goes from the focused element up to the root; the deepest handler wins unless it calls
  `cx.propagate()`. `capture_action` runs root → leaf (rarely needed).
- Workspace-level actions (open, close tab, toggle panel) are registered on the workspace from each
  crate's `init` via `cx.observe_new(|workspace: &mut Workspace, _, _| workspace.register_action(…))`.
- App-level actions (quit, open settings) use `cx.on_action(...)` in `init`.
- Buttons: `.on_click(|_, window, cx| window.dispatch_action(SendRequest.boxed_clone(), cx))` or call the same method via `cx.listener`.

## 3. Bind

Keymaps are JSON arrays of `{ "context": "...", "bindings": { "keys": "namespace::Action" } }` in
`assets/keymaps/default-macos.json`, `default-linux.json`, `default-windows.json`:

```json
[
  {
    "context": "RequestEditor",
    "bindings": {
      "ctrl-enter": "request_editor::SendRequest",
      "escape": "request_editor::CancelRequest",
      "ctrl-shift-f": "request_editor::FormatBody"
    }
  },
  {
    "context": "Workspace",
    "bindings": {
      "ctrl-1": ["workspace::ActivateTab", 0],
      "ctrl-shift-p": "command_palette::Toggle",
      "ctrl-e": "environment_picker::Toggle"
    }
  }
]
```

- Context predicates: `"Picker > Editor"` (descendant), `"menu || Picker"`, `"!InlineEdit"`, `"RequestEditor && mode == raw"`
  (set key/value pairs on the element's `KeyContext`).
- Multi-stroke: `"ctrl-k ctrl-s"`. macOS uses `cmd-`, Linux/Windows `ctrl-`; write each file explicitly
  rather than relying on aliases.
- Loading: parse the JSON at startup, build actions with `cx.build_action(name, data)`, create bindings with
  `KeyBinding::load(keystrokes, action, context_predicate, use_key_equivalents, action_input, cx)`, and
  `cx.bind_keys(bindings)`. Report bad entries to the user (notification), don't panic — user keymaps
  (`<platform config dir>/pustman/keymap.json`) layer on top of defaults.
- Tests: `cx.bind_keys([KeyBinding::new("ctrl-enter", SendRequest, Some("RequestEditor"))])` (static strings only).

## 4. Conflict check (every new binding)

1. Grep all three keymap files for the key string.
2. Avoid OS/WM-reserved keys (macOS cmd-H/cmd-Q/cmd-M; GNOME super-*; Windows win-*).
3. Prefer Postman-familiar bindings where they exist (send = cmd/ctrl-Enter, new tab = cmd/ctrl-T, save = cmd/ctrl-S, find = cmd/ctrl-F, command palette).
4. Show the binding in the action's tooltip and menu entry (our `ui` crate's tooltip-for-action helper, modeled on Zed's `Tooltip::for_action`).

## Traps
- Handler not firing → element lacks `track_focus`, focus is elsewhere, or a deeper context binds the same key.
- Action type not registered → it's never referenced; `actions!` registers automatically when the crate is linked, but make sure the crate is initialized in `main`.
- Data actions without `deny_unknown_fields` silently ignore typos in user keymaps.
