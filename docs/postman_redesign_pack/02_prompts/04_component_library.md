# 04 · Build reusable components

**Inputs:** Design system spec, component inventory, approved foundations.  
**Scope:** `04 Components`. Shared-system owner only.  
**Review gate:** Instances and states survive realistic content.

```text
Follow the operating contract. Build the component inventory from design_system.md using the existing variables. Work in small related groups and inspect each group before expanding.

Cover the app shell, navigation item, collection/request row, request tab with unsaved state, method selector, URL field, Send/Cancel control, environment selector, key/value editor row, content tabs, editor shell, response summary, status message, empty state, menu, dialog, toast, member row, role/capability indicator, comment thread and change-review summary where required by the spec.

Use actual components, component properties and instances. Use variants for meaningful state changes; use text/boolean/instance-swap properties for content where appropriate. Avoid a full combinatorial variant explosion. Keep text and icons independently editable. Use an existing consistent icon set or approved assets; do not rasterize control labels.

For each interactive component show applicable default, hover, focus-visible, active/selected, disabled, loading and error states. Document the behavior expected on keyboard activation and where focus moves after an overlay closes. A disabled state must have an understandable reason when the task requires it.

Stress examples with long names, 30-row tables, missing avatars, large counts, multilingual names and a narrow container. Verify shared changes propagate to instances. Add a usage sheet identifying when to use each component and common mistakes.

Completion criteria: reusable structure is real; tokens are bound; required states are represented; long content is handled; and all screen builders can find the canonical components without guessing. Report any proposed additions to the inventory separately.
```
