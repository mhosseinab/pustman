# 06 · Design auth, variables and debugging states

**Inputs:** Screen specs, state matrix, domain rules, fixtures, canonical workbench.  
**Scope:** Related request states on `07 Edge states`.  
**Review gate:** Each error has correct meaning and a clear next action.

```text
Follow the operating contract. Extend the canonical workbench with the auth/environment/debugging states required in the specs. Use the defined state IDs and reuse the existing structure.

Include auth inherited from a parent, request-level auth override, missing credentials, a received HTTP 401 response, a received HTTP 500 response, network/transport failure, timeout, in-flight cancellation, malformed input and an unresolved variable. Do not give a transport failure a fictional HTTP status code. Do not present 401 as proof of one specific cause unless the response fixture says so.

Show effective variable values with their source/scope where the spec requires it. Keep unresolved, overridden and sensitive values visually distinct. Preserve the supplied precedence and local/shared semantics; do not simplify them into an invented single environment value. Secret-like values remain masked and are excluded from shared review/diff surfaces unless the spec explicitly defines safe placeholders.

For every error state, show the local cause/evidence, any retained user input, the primary recovery action and the result after recovery. Display inherited configuration in a way that explains where edits belong. Maintain clear active-environment context throughout recovery.

Add focused before/after frames for fixing an unresolved variable and recovering from the provided 401 scenario. Keep destructive or environment-changing operations within the spec's safeguards.

Completion criteria: all listed states exist, status meanings are correct, recovery preserves user work, secret handling matches the spec, and unresolved questions are recorded rather than filled with invented behavior.
```
