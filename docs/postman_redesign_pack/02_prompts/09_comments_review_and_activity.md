# 09 · Design comments, review and useful activity

**Inputs:** Collaboration spec, review/comment fixtures, screen specs, canonical workbench.  
**Scope:** Contextual feedback and review frames on `06 Team collaboration`.  
**Review gate:** Team discussion stays attached to concrete API work.

```text
Follow the operating contract. Design the scenario where Alex asks Mina to review a request change in Commerce API. Use the review model, capabilities and exact status semantics specified in collaboration_spec.md. If an approval/change-request model is a proposed feature, label that as a proposal in the design notes; do not portray it as existing Postman behavior.

Show the entry point from the request or collection, the artifact/change being discussed, the relevant before/after evidence, reviewer selection, a comment thread and the review outcome. Distinguish a comment, a review decision and the action that applies a change. Do not allow a visual approval badge to imply a change was merged or published when it was not.

Keep discussion tied to the specific artifact and, where specified, revision. Include a mention, unread/new activity, resolved/reopened thread and a stale/outdated review state where defined. Explain what changed after the reviewer began reading. Use synthetic identities and contents from the fixtures.

Design a small useful activity/inbox surface that returns users to the affected API work. Avoid a generic social feed or full-screen chat taking over the workbench. Reflect only events the spec defines. Respect Sam's capabilities on viewing, commenting, resolving and reviewing.

Completion criteria: Alex can request feedback, Mina can inspect and respond in context, the outcome is unambiguous, stale content is identified, and notifications lead back to the correct resource/state without revealing inaccessible content.
```
