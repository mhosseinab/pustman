# 13 · Review the design, then fix specific issues

**Inputs:** Approved frames/prototype, specs, state matrix, task scenarios.  
**Scope:** Review first; make scoped edits in a second pass.  
**Review gate:** Findings have evidence, severity and a verification path.

## Review prompt

```text
Review the named frames and prototype as a developer completing the supplied scenarios, as a reviewer collaborating on API work, and as a viewer with restricted capabilities. Do not edit the designs in this pass.

Check task continuity, information density, environment/variable clarity, inherited auth, unsaved work, response/test semantics, permission scope, stale reviews, conflict recovery, secret exposure, resize behavior, keyboard focus and contrast. Inspect actual screens and run actual prototype tasks where possible.

For each finding return: issue ID; frame/component or prototype step; expected behavior from the spec; observed evidence; user impact; severity; proposed fix; and how to verify it. Distinguish a demonstrated defect from an untested hypothesis. Do not invent testing with real people or treat an agent persona review as user research.

Prioritize issues that stop task completion, risk losing work, mislead users about permissions/results, or expose sensitive values. Also record what could not be tested. Finish with the smallest set of changes that resolves the serious issues without changing the approved direction.
```

## Scoped revision prompt

```text
Fix only these reviewed issues: [ISSUE IDS]. Use the review's evidence and the existing specs. Keep unrelated screens and approved navigation stable.

Use existing variables/components. If a shared-component change is required, list affected screens, make the smallest valid change and recheck those instances. Add no unrelated styling or features.

After editing, rerun the specific verification steps for each issue at the required size/theme. Report resolved, partially resolved and unresolved issues with actual evidence. Update the decision log and state matrix only where behavior changed. Do not declare the entire product tested based on these targeted checks.
```
