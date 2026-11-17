# 10 · Design conflicts and shared-environment changes

**Inputs:** Collaboration spec, conflict/shared-environment fixtures, state matrix.  
**Scope:** Team conflict and environment-safety frames on `07 Edge states`, linked from `06 Team collaboration`.  
**Review gate:** No silent data loss or accidental secret sharing.

```text
Follow the operating contract. Design two collaboration scenarios from the spec: concurrent changes to a shared request and a change to shared environment configuration.

For the concurrent-change scenario, show the version/context Alex edited, the incoming change from Mina, the point at which the conflict is detected, and the available resolution paths. Follow the collaboration model in the spec: do not invent real-time co-editing, branching, locking or automatic merge behavior. If a proposed resolution feature is being explored, mark it clearly as a proposal in the notes.

Preserve both sets of useful work until a resolution is chosen. Identify changed fields and the resulting artifact. Make keeping local edits, accepting the incoming version, or applying a manual resolution distinguishable where the spec permits them. Include a cancelled resolution and a permission-revoked recovery.

For shared environments, make the target environment, affected scope, effective permission and impact of the proposed change clear. Distinguish a person's local value, a shared value and secret material using the specified model. Never show secrets in activity, diff, notification or review examples. Use safe fake placeholders.

Show the before/after state, any required review or confirmation, and the evidence that the change succeeded or failed. Do not add a confirmation to every harmless edit; use the consequence-based rules in the spec.

Completion criteria: the design explains who changed what and where it applies, preserves work during conflict, handles cancellation/failure, and supports the specified environment collaboration model without inventing backend guarantees.
```
