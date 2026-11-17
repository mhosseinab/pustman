# 05 · Build the canonical request workbench

**Inputs:** Approved layout, request screen specs/state matrix, fixtures, components.  
**Scope:** Canonical request-workbench frames on `05 Workbench`.  
**Review gate:** The main developer task remains obvious with real data.

```text
Follow the operating contract. Build the canonical request workbench specified in screen_specs.md at 1440×900, using existing component instances and the selected layout direction. Use the Orbit Labs / Commerce API fixtures exactly.

Show the intended workspace/project context, searchable collection tree, multiple request tabs, an unsaved indicator, active environment, HTTP method, full editable URL, Send, request configuration tabs, key/value editing and the response area. Use the panel arrangement and labels from the spec. Keep team context available through the specified control without giving it more visual weight than the request task.

Build the initial/no-response, successful-response, and unsaved-change states listed in the state matrix. Use the nested JSON response, elapsed time and HTTP status from the fixtures. Display response size only if it is supplied; otherwise omit it or show Not recorded. Do not invent a transfer size. Preserve the distinction between response status, network failure and test outcome.

Add a compact behavior annotation for keyboard navigation, pane resizing/collapse, tab close with unsaved work, saving location and active-environment changes. Do not invent shortcuts that conflict with existing conventions in the supplied spec. Treat a proposed shortcut as a proposal until confirmed.

Create the 1280×800 layout variant. Check the long URL, long request name, collection depth, response editor and open team-context panel. Keep critical actions visible and document which sections collapse or scroll.

Completion criteria: task-critical information is discoverable, every repeated control is an instance, fixture data is accurate, two widths work, and a reviewer can tell what happens before Send, after success, and before abandoning edits.
```
