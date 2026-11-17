# 14 · Prepare engineering handoff

**Inputs:** Final designs/prototype, all specs, decisions, state matrix, QA results.  
**Scope:** `09 Handoff` and supporting Spec/Markdown.  
**Review gate:** Another engineer can implement without guessing behavior.

```text
Prepare the redesign handoff using the approved design and prototype. Do not change the visual direction during this task.

Create a traceability table mapping requirement and scenario IDs to screen/frame links, component names, states, behavior rules and QA evidence. Use real links/IDs from the file, and mark missing ones instead of inventing them.

Document layout and resizing rules, semantic tokens/modes, typography, component properties, interaction outcomes, keyboard/focus behavior, loading/error/empty states, data examples and permission-sensitive actions. Keep implementation-only notes in handoff annotations rather than user-facing product copy.

For collaboration, identify the resource and revision being reviewed, effective capabilities, membership scope, conflict strategy, local/shared environment semantics and required authorization checks. Distinguish a prototype simulation from an implemented backend guarantee.

List reusable components in priority order for implementation. If production components already exist and Code Connect is available, propose precise mappings and identify prerequisites. Do not claim mappings are saved, components published or code generated unless those actions actually occurred.

Capture open decisions, remaining QA risks and deliberate exclusions from this milestone. Include the complete critical flow and the next expansion scope. Preserve a copy of accepted design decisions and keep the product/spec/prototype relationships explicit; do not assume automatic two-way synchronization.

Completion criteria: every critical requirement maps to inspectable output, states and permissions are explicit, remaining gaps are visible, and the handoff describes the design that was actually built.
```
