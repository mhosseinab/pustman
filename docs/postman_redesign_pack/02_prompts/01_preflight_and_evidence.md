# 01 · Check capabilities and audit evidence

**Inputs:** Operating contract, product brief, collaboration spec, screenshot register, 3–5 relevant screenshots.  
**Scope:** `00 Brief and decisions`, `01 Evidence`, temporary `Capability check`.  
**Review gate:** Editable structure works; evidence and proposals are distinguishable.

```text
Follow the operating contract. First inspect the current file and list the useful libraries, existing pages and available design capabilities. Do not claim a capability was tested unless you actually exercised it.

Create a temporary Capability check page. Build one native request-row component with a method badge, request name, secondary path and selected indicator. Use auto layout, editable text and semantic color variables. Add default, hover, selected and focus-visible states, then place two instances. Change a main-component spacing value and verify both instances update. Test one light/dark variable-mode change. Report unsupported actions and the route we should use for them.

In 01 Evidence, place the supplied product screenshots with their IDs, source URLs, retrieval dates and version limitations. Preserve their labels as current-product references. Do not label them as designs we created. If import is unavailable, create a source-index section and explain the missing step.

Write an evidence table with four columns: observed behavior and source; functionality to preserve; redesign hypothesis; evidence needed to validate it. Include the request lifecycle, environment/variable visibility, ownership and permissions, collaboration feedback, and recovery from failure. Do not infer behavior solely from an icon or a static screenshot.

Create a brief decision log identifying scope choices, evidence gaps and proposed features. The desktop layout sizes, sample team, and design directions are project choices, not claimed Postman requirements.

Completion criteria: the canary uses actual components and variables; each reference has provenance; every asserted observation has a source; proposals are explicitly labeled; and we have a workable design route for the next task. Report the result, then stop this task.
```
