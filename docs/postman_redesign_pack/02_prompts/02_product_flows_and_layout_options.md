# 02 · Model the product and compare layouts

**Inputs:** Product brief, screen specs, collaboration spec, scenarios, evidence findings.  
**Scope:** `02 Flows and IA`.  
**Review gate:** Choose one direction using the same content and tasks.

```text
Follow the operating contract. Translate the supplied brief into an information architecture and six task flows:
1. Open/import a request, choose the environment, send and inspect the response.
2. Diagnose an auth or unresolved-variable failure and retry.
3. Save a request in the intended team collection without losing local changes.
4. Run tests and inspect a failed assertion.
5. Ask Mina to review a proposed API-request change, discuss it and resolve it using the collaboration spec.
6. Let Sam inspect shared work with the effective capabilities assigned to Sam; show a meaningful recovery when an action is unavailable.

Represent flow steps as editable notes/tables on the canvas or in a Spec. If the current agent cannot draw a proper flow diagram, do not fake it as a single decorative image; use a structured task table for now.

Create the three grayscale 1440×900 alternatives defined in product_brief.md with identical fixture content and capabilities: A focused workbench with request above response; B side-by-side request and response inspector; C team project hub with a clear route into the workbench. For C show the hub and its workbench destination so the same execution and review tasks can be compared. Each workbench must show navigation, request tabs, active environment, method/URL/Send, request editing, response evidence and an optional team-context panel. Collaboration should support the API task, not displace it with a chat dashboard.

For each option show how it behaves at 1280×800. Annotate the trade-offs for first-use clarity, repeated developer work, debugging, team context and long content. Keep labels, data and feature coverage comparable so visual differences do not bias the comparison.

Recommend one option with explicit reasoning, but label it as an unvalidated design recommendation. List which later screen families it can support. Do not build all high-fidelity screens before the direction is selected.

Completion criteria: six task flows with failure/recovery steps; three comparable editable alternatives; no orphan top-level navigation; and a documented recommendation.
```
