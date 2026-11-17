# Design review and agent acceptance checklist

Record evidence per item. Mark Pass, Fail, Not tested, or Not applicable; never turn an untested item into a pass.

## Native Figma structure

- Text and controls are native editable elements. There is no full-screen raster hidden inside the final frame.
- Repeated rows, buttons, fields, tabs, badges and comments use component instances.
- Spacing and resizing follow intentional auto-layout rules; inspect 1440 x 900 and 1280 x 800.
- Semantic variables switch light/dark modes; text styles and fonts are consistent.
- The component owner can change one main component and see all relevant instances update.
- Main, alternate and deprecated frames have meaningful names; obsolete work stays in Archive.

## Request behavior

- Active environment, resolved URL and inherited auth source are visible or one clear action away.
- Query editing preserves repeated keys and disabled rows; raw URL and parameter table agree.
- Saved request, current dirty editor and execution snapshot are distinct.
- Response state distinguishes transport, HTTP and assertions; no invented status for a timeout.
- Missing variables, empty body, binary body, huge payload, failed scripts, cancellation and retry have deliberate behavior.
- Diagnostics keep useful raw information and identify whether logs are local.

## Team collaboration

- Workspace access, resource access, API authorization and secret access are distinct.
- A viewer's personal environment value can be editable while shared values remain protected.
- Invitation pending, accepted, expired, failed and revoked states are distinct.
- Copying a private link or mentioning a teammate does not grant access.
- Share previews identify actual audience and transmitted data; masking is not labeled as exclusion.
- Review assignment does not imply merge authority; approval does not imply completed merge.
- The run evidence identifies the request revision/environment or explicitly says the link is unknown.
- Conflicts have a visible result preview and no silent overwrite; target changes during resolution are handled.
- Local/Git and cloud fork workflows use accurate terms and do not imply identical comments/review support.
- Offline or permission-revoked states preserve local work without claiming it is shared.
- Avatars do not imply live co-editing unless that behavior is actually implemented.

## Accessibility and long content

- Intended foreground/background pairs meet the documented contrast target. Token checks alone do not cover every rendered combination.
- Focus is visible, unobscured and follows the task; dialogs return focus correctly.
- Icon-only controls have accessible names and usable targets. Status is not conveyed by color alone.
- Trees, tabs, menus, dialogs and tables have correct keyboard behavior in the runnable prototype.
- Dynamic status changes are announced appropriately; error messages are linked to inputs.
- Long URLs, 80-character request names, large JSON, long team names, many members and small windows remain usable.
- Reduced motion, zoom and native-platform accessibility needs are tested where applicable.

## Prototype truthfulness

- Every primary visible action works or explains its incomplete state.
- The scenario/persona selectors and simulated network/collaboration are clearly marked as demo controls.
- Reset restores a deterministic baseline; persona switching does not leak another person's local value.
- No real API, email, invitation, production credential or actual team setting is required for the demo.
- Final report separates visual validation, functional simulation and production capabilities not tested.

## Handoff

- Each requirement links to a screen/state, relevant component and acceptance check.
- Decisions, assumptions and deferred scope are explicit.
- Review findings point to a concrete frame/component and observed defect.
- Design and runnable prototype versions are identified; no automatic round-trip sync is assumed.
