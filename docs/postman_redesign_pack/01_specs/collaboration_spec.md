# Team collaboration specification

Status: proposed UX and acceptance criteria. Read source_register.md for verified Postman behavior. The design uses current workspace/resource concepts but does not assert that every proposed feature exists today.

## Collaboration is a core product area

The app must support discover -> understand -> run -> change -> review -> merge -> reuse. Members management alone is not sufficient. Keep ownership, audience, effective access, version context, and recovery visible throughout the flow.

Primary objects: team; workspace; member/invitation; collection; folder/request/example; environment; secret reference; comment thread; cloud fork/proposal; review; run result; version; Git repository/branch/commit when using the separate Git route.

## Permission vocabulary

Workspace roles include Admin, Editor and Viewer in current Postman documentation. Team roles, workspace roles and resource-specific roles are separate. Do not equate workspace Admin with organization Super Admin or assume a Viewer badge describes every resource permission.

The design must show the effective permission returned for the action and its source, for example: "Can edit - inherited from workspace". When an individual resource grant combines with workspace access, use the product's verified rule. Do not invent a deny override or permission intersection.

The following is the presentation contract, not a replacement for the complete backend authorization model:

| Action | Required UI behavior |
| --- | --- |
| View a shared artifact | Show audience, artifact location and effective access |
| Run a request | Separate access to the request from authorization against the API; workspace membership does not supply a token |
| Edit shared source | Enable only when effective permission allows it; otherwise explain the source of the restriction and offer an allowed path |
| Comment or review | Resolve actual comment/review capability; do not infer it solely from edit permission |
| Manage members or visibility | Resolve the action's authorization, show before/after effects, and confirm only meaningful audience changes |
| Merge changes | Require actual target edit/merge authority plus any configured policy; approval and merge are distinct states |
| Manage secrets | Treat secret-provider access separately from collection/environment visibility |
| Change organization settings | Keep organization-level authority separate from workspace administration |

Fixture permissions are intentionally explicit booleans in 04_fixtures/team_permissions.json. They describe the demo scenario, not all Postman plans. An implementation must obtain permissions from its real authorization source.

Verified nuance: an environment Viewer may edit their personal local value while the shared definition and shared values remain read-only. Do not disable the entire table. Reviewer is a review assignment, not an additional workspace role.

## C1. Team workspace overview

Entry points: workspace switcher, invitation, shared link, or inbox notification.

Show purpose, owner/admin contact, visibility, collections, suggested first request, environment setup checklist, recent meaningful changes, and pending reviews. Use the existing workbench directly for returning users; retain an easy route to this overview.

States: loading; no collections; populated; no access; archived; offline snapshot; partially available resources.

Acceptance: a teammate can identify what this workspace is for, what to run first, and who can grant access. Activity entries contain an actor, action, object and timestamp; failed actions must not appear as successful activity.

## C2. Members and invitations

The members view separates current members from pending invitations and access requests. Invite accepts fictional emails such as sam@example.org in prototypes. Display chosen workspace, audience, role description and whether the change may affect a paid seat; derive any real pricing from the current product instead of inventing it.

States: invalid address; already a member; invitation pending; resent; expired; revoked; accepted; failed; no permission; account mismatch; changed access while modal is open.

Acceptance: an invitation is not labeled accepted before the invitee acts. Changing role does not imply sending an email unless the system actually does so. A failed send retains the entered addresses. The prototype simulates sending and clearly identifies its demo mode.

## C3. Sharing and visibility

A Share action opens a preview containing the artifact, destination workspace/audience, effective roles, fields included and any excluded local values. Visibility labels must use the correct current Postman taxonomy for the chosen target build. Do not invent link-access behavior.

Never treat possession of a copied link as proof of access. For a private resource, a recipient without access sees a request-access route. Mentions also do not grant access.

For changing to a broader audience, show precisely what becomes accessible. Keep normal private link copying lightweight. Do not add the same confirmation to every low-impact action.

Environment and run sharing need a data preview. Secret masking and exclusion are different: a masked shared value may still be retrievable by a permitted user, while an excluded local value is not transmitted. The UI must not claim masking alone makes a value private.

## C4. Comments, mentions and notifications

Comments belong to an artifact and optionally a supported field/change anchor. A contextual panel shows thread status, author, timestamp and anchor context. Use inline comments only where the target product supports them; use artifact-level threads elsewhere.

Current documentation restricts comments in Local View for Git-connected workspaces until changes are synced to Cloud. Design this as an explained unavailable state, with an allowed cloud route; do not simulate local comments as an existing product guarantee.

Proposed improvement: show a revision chip when a comment or review refers to an earlier revision. Label an unavailable anchor "Original context changed" and provide the captured diff/snapshot if available. This requires version metadata and is not assumed to exist in current Postman.

Mention suggestions must distinguish members who can access the artifact. A mention to someone without access should explain that access is still needed. Draft text survives network failures and navigation; the app must not post a comment twice after a retry.

Notification inbox: mentions, review requests, review outcomes, access decisions and relevant failures. Keep read/unread distinct from resolved/unresolved. Opening a notification lands on the correct artifact/thread and revision if retained.

States: empty thread; posting; posted; post failed with retry; edited; resolved; reopened; deleted; access lost; anchor stale; mentioned person lacks access.

## C5. Cloud fork and change review

Label the model "Postman fork" or equivalent current terminology. Show parent collection/environment, fork label, source workspace, target workspace and comparison basis.

Use an HTTP-only collection for this first review fixture. Current fork documentation has limitations on pull/merge/conflict behavior for collections containing non-HTTP requests; protocol parity requires a separate verified decision.

Proposed review layout: changed-object tree, before/after diff, summary and test evidence, comments, and a persistent review status. Do not use source-control words interchangeably when the underlying objects differ.

Review states: draft; open; review requested; feedback given; changes requested (if supported by the workflow); approved; conflicts; merge unavailable; merged; closed. If a desired state is a project proposal rather than current Postman behavior, document it explicitly and model it in the prototype only.

Important distinctions:

- A 200 response is not a passed test run.
- A green run refers to an environment and revision; it is not timeless approval of the collection.
- Review approval is not merge completion.
- Editor access may allow direct merge in current Postman; a mandatory approval gate must be a real policy or an explicitly proposed addition.
- Fresh edits may make earlier evidence stale. If the system cannot prove the relationship, show "Revision not recorded" instead of inventing a verified status.

Acceptance: an authorized user can determine what changed and what the run tested without opening an unrelated dashboard. A viewer does not see an enabled merge action merely because a review was approved.

## C6. Conflict resolution

For each conflict, show source and target values, which object/field changed, and a preview of the resulting artifact. Use a three-way base/ours/theirs view only when a common base is actually available. Otherwise describe a two-way conflict honestly.

Keep an original copy before resolution. Require each unresolved conflict to be addressed before completing the merge. Validate merged JSON/scripts, and show a new conflict if the target changes during resolution. Do not silently select last-writer-wins.

States: conflict list; item unresolved; source selected; target selected; manually combined; invalid content; target advanced; permission revoked; merge failed; merged.

Acceptance: neither side's edit disappears without a visible decision. After success, show the resulting target revision and clear stale status. If merge fails, preserve the resolution work for retry.

## C7. Local/Git work and team sharing

Use separate labels for local save, commit, push, cloud publication and review. The exact operations vary by Postman version and repository configuration. Do not show a single green "Synced" badge as proof that all operations succeeded.

A Git context strip may show repository, branch, pending changes and remote status. A cloud context strip may show workspace, shared artifact revision and fork status. Keep comments and review links tied to the appropriate model. Git provider review may open the provider if that is the actual supported route.

States: local changes; committed but unpushed; remote unavailable; credentials needed; behind remote; conflict; pushed; cloud snapshot outdated; private local-only resource.

These statuses are a design inventory. Implementation must only display states the backend can establish.

## C8. Shared environments and secrets

Treat variable scope, local/shared value, and secret storage as three axes. "Local variable scope" is not the same thing as a variable's local value. The variable inspector shows definition source, effective value, shadowed alternatives and sharing status without revealing secrets by default.

Example: base_url is shared in the Development environment; Alex's access_token is a local override or a vault reference; Sam must configure their own credential. Publish and review previews exclude local values by default and show exactly which shared values are included.

For a direct Local Vault reference, current syntax is `{{vault:name}}`. Shared Vault uses a secure variable reference such as `{{api-key}}`; storing a shared secret, sharing the variable that refers to it and granting secret access are separate operations. A shared-environment Viewer role does not automatically authorize every shared secret.

Design empty, missing, expired, revoked, unavailable-provider, permission-denied and value-shadowed states. Secret reveal must follow the real permission model. Never place a real token in a Figma file, screenshot, comment fixture or published prototype.

## C9. Offline and access-loss recovery

The workbench preserves unsent and unsaved local edits. Offline mode must distinguish inability to reach collaboration services from inability to reach the target API; a local API may still be available.

If edit access is revoked mid-session, block shared writes, explain why and preserve a permitted local copy/export path. Do not falsely report that the draft is saved to the team. On reconnect, compare versions before replaying changes. A posted comment must have a stable ID to avoid duplicate retries.

## Paired-team validation script

1. Jo selects Viewer for Sam's invitation and sends the simulated invite; access starts after Sam accepts.
2. Sam joins, finds List orders and configures their own local token.
3. Alex changes limit from 20 to 50 in a fork, preserves status=processing, and attaches a run summary.
4. Mina identifies the change and asks Alex to keep the pagination assertion. Alex confirms the change applies only to this saved request.
5. Another target edit creates a conflicting value of 100. Alex resolves to 50 after review; neither 20 nor 100 should vanish without explanation.
6. Mina merges only when the effective permission and configured policy permit it.
7. Sam sees the new shared artifact, can run it, and correctly explains why editing shared source is unavailable.
8. Revoke Alex's shared edit permission while they have a dirty draft; verify draft preservation and truthful status.

Record task completion, misunderstanding of audience or role, stale evidence used for approval, secret-sharing mistakes, unrecoverable edits, and comments lost or duplicated. Compare against the current workflow before claiming improvement.
