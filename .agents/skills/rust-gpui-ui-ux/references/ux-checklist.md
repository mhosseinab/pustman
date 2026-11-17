# UX quality checklist

Adapted from Zed's `docs/src/development/ui-checklist.md` and `feature-process.md`, extended for an API
client. Walk it before calling UI work done; report which items you verified and how.

## Input & discoverability
- [ ] Every new command is an action, has a keybinding (checked for conflicts on all 3 platforms), appears in the command palette.
- [ ] Bindings are shown in tooltips and menus.
- [ ] Fully usable without a mouse: reachable by Tab/arrow keys, Enter/Space activate, Escape backs out.
- [ ] Right-click menus exist where users expect them and mirror keyboard actions.

## Visual
- [ ] Correct in light and dark themes; only theme tokens/semantic colors used.
- [ ] Focus is clearly visible (focused pane border, `focus_visible` on controls).
- [ ] Works in narrow (320px pane) and short windows; text truncates with tooltip, nothing overlaps or clips controls.
- [ ] Scales with UI font size and density setting (Compact/Default/Comfortable); crisp on HiDPI.
- [ ] Popovers/modals stay within window bounds.
- [ ] Spacing, alignment and terminology consistent with the rest of the app ("Collection", "Environment", "Variable", "Request" — never synonyms).

## States
- [ ] Empty state teaches the next action.
- [ ] Loading: instant feedback; spinner/progress for >100 ms; elapsed time for requests; cancellable.
- [ ] Errors are actionable (what happened, why, what to do) and appear near the cause; nothing fails silently.
- [ ] Unhappy paths: offline, DNS failure, TLS error, timeout, 4xx/5xx, huge response, binary response, invalid JSON, corrupt/missing collection file, permission denied, unresolved variables.
- [ ] Destructive actions confirm or are undoable; unsaved changes never lost on close.
- [ ] Progressive disclosure: advanced options (proxy, TLS, redirects, scripts) don't clutter the default view.

## Accessibility
- [ ] Every interactive element has a role and label; icon buttons have tooltips + labels.
- [ ] Meaning never conveyed by color alone.
- [ ] Async results announced.

## Performance
- [ ] No frame > 8 ms with realistic worst-case data; long lists virtualized; heavy work off the main thread.

## Persistence & settings
- [ ] UI state that users expect to survive restart does (open tabs, dock sizes, selected env, scroll positions where cheap).
- [ ] New settings have defaults in `assets/settings/default.json`, a schema, docs, and apply live.

## Security & privacy
- [ ] Secrets never shown in logs, tooltips, console, history exports; masked by default with reveal toggle.

## Platforms
- [ ] Checked (or explicitly noted as unchecked) on macOS, Linux (Wayland + X11), Windows; platform-correct modifiers (cmd vs ctrl) in labels.
