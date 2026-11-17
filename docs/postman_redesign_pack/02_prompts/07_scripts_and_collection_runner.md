# 07 · Design scripts, tests and runner results

**Inputs:** Script/runner screen specs, state matrix, fixture scripts/results.  
**Scope:** Script editor and runner screens on `05 Workbench`; exceptional states on `07 Edge states`.  
**Review gate:** A developer can locate and understand a failed check.

```text
Follow the operating contract. Build the scripts/test and collection-runner screens specified in the brief. Use realistic fixture scripts and results instead of placeholder lorem ipsum or invented API outcomes.

Show the script stage/context, editable code area, available run controls and useful diagnostics. Distinguish a script runtime error from an assertion failure and from a request's HTTP result. If script inheritance or execution order is relevant, represent only the order defined in the spec and cite the source in a behavior annotation.

For the runner, include setup, running, completed, failed and cancelled states from the state matrix. Show the selected collection/folder, environment, request order and the defined iteration/data options. Include a clear summary and a drill-down from a failed result to the request, assertion and observed/expected details.

Use a mixed result fixture containing a successful request with a failed assertion. Verify the design does not equate a 2xx response with a passing test suite. Include long request names, many iterations and a meaningful empty-state explanation.

Integrate the collaboration rule for sharing a run result or asking a teammate to investigate. Make the shared artifact and its visibility clear; don't accidentally imply a local secret or private environment value is included.

Completion criteria: run configuration is understandable, progress/cancellation is visible, the failed assertion is easy to locate, permission-sensitive sharing follows the spec, and the result-to-request navigation is documented.
```
