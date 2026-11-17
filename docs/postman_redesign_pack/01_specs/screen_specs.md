# Screen specifications and state contract

Version 1.0 · Prepared 17 November 2026 · Proposed redesign, not a statement of shipped Postman behavior.

This specification covers **28 screen families and 106 concrete state rows** in [screen_state_matrix.csv](screen_state_matrix.csv). The first 26 families specify the individual and team workflows in milestone 1. S27 and S28 are later coverage maps; they do not imply detailed full-product parity.

Read [product_brief.md](product_brief.md), [collaboration_spec.md](collaboration_spec.md), and [design_system.md](design_system.md) together. Use the fixtures in `../04_fixtures/` as the data source. The scenario is Orbit Labs / Commerce API: Alex is an Editor, Mina is an Editor assigned to review, Sam is a Viewer with their own personal local credential, and Jo is an Admin. Reviewer is an assignment, not an invented fourth permission role.

## Stages and priority

| Code | Meaning |
| --- | --- |
| M1-P0 | Build in the first complete individual-and-team prototype; required for the two golden paths. |
| M1-P1 | Complete in the next pass of the same milestone before calling milestone 1 finished. |
| M2-map | Inventory and further specification for protocol-specific workflows and automation; not detailed parity yet. |
| M3-map | Inventory and further specification for organization and ecosystem surfaces; not detailed parity yet. |

Requirement IDs R01–R28 are this specification's stable traceability IDs. The CSV repeats the matching ID on every state row. They are project requirements, not identifiers from Postman's documentation.

## Shared layout and interaction contract

Use editable component instances and the proposed token system. Start at 1440×900, verify 1280×800, and include the defined light/dark modes. Follow `05 Workbench`, `06 Team collaboration`, and `07 Edge states` in the canonical page structure. Most states are variants or panels, not new top-level destinations.

The URL, method, environment, request identity and Send action stay discoverable. Comments use a contextual panel; member management, review and conflict resolution use the main area. Collapse secondary regions before narrowing code below useful readability. Use a unified diff when two columns no longer fit. Keep navigation state and pane sizes personal.

Every reusable control also needs applicable default, hover, focus, selected, disabled-with-reason, and loading behavior. These mechanical component states are not duplicated as full-screen CSV rows. Annotate focus order, dialog focus return, keyboard behavior, asynchronous announcements and error associations. Verify them in the runnable prototype; a static frame does not prove accessibility.

## Response snapshot versus current editor: required behavior

Maintain three separate concepts in the proposed model:

1. **Current editor draft:** the request configuration being edited now, with an unsaved/saved status.
2. **Execution context:** a captured representation of the method, URL/parameters, environment identity, relevant configuration and revision at the moment Send was invoked, plus an execution ID and timestamp.
3. **Execution result:** response data, transport outcome and assertion results attached to that execution.

An environment change, variable edit, auth change or body edit must never rewrite an already displayed response or test result. Show **Response from previous request settings** when the current draft differs from the captured context; expose the previous environment and timestamp. A fresh Send creates a new result, while the preceding result remains visibly historical if retained.

The execution capture must not persist secret values merely to support a visual comparison. Keep safe references or redacted diagnostic fields and label omissions. If the backend cannot establish a revision relationship, show **Revision not recorded** instead of claiming the result tested the current draft.

History opens a read-only historical view or a separate tab. **Open request settings as new draft** is an explicit action. A dirty current draft is retained unless the user explicitly chooses replacement. Sharing a response or run creates a preview of the available captured data and audience; it is not a live link to the sender's latest editor values. Define snapshot retention and access before implementation.

## Execution and permission invariants

- Transport completion, HTTP response status and assertions are independent. Keep 200 with a failed assertion, expected 401 with a passed negative check, no-response transport failure, 204 with no body, and canceled-wait states distinct.
- Canceling a client wait does not guarantee that a server-side mutation stopped or rolled back.
- Switching an environment never sends a request automatically.
- Workspace access does not provide API credentials.
- Use the effective action capabilities and their source. Do not infer every action from an Admin, Editor or Viewer label.
- Sam may change a permitted personal local value while the shared environment remains read only.
- Ordinary copied links and @mentions do not grant access.
- Review approval, merge, Git push and cloud synchronization are distinct outcomes.
- Show successful writes, invitations, comments and merges only after the relevant acknowledgment. A prototype simulates these actions and does not send real messages.
- Preserve a permitted local draft after failures. Do not offer export/copy as a bypass when current policy forbids it.

## Screen index

| Screen | Family | Stage | Requirement |
| --- | --- | --- | --- |
| S01 | Workspace overview and switcher | M1-P0 | R01 |
| S02 | Workbench, collection tree and tabs | M1-P0 | R02 |
| S03 | HTTP composer and data editors | M1-P0 | R03 |
| S04 | Send and execution outcome | M1-P0 | R04 |
| S05 | Response inspection and freshness | M1-P0 | R05 |
| S06 | Authorization and inheritance | M1-P0 | R06 |
| S07 | Environment selection and variable resolution | M1-P0 | R07 |
| S08 | Environment editor and secret references | M1-P0 | R08 |
| S09 | Scripts and individual test results | M1-P0 | R09 |
| S10 | Collection overview and organization | M1-P1 | R10 |
| S11 | Collection Runner setup | M1-P1 | R11 |
| S12 | Collection run progress and results | M1-P1 | R12 |
| S13 | History and response snapshots | M1-P1 | R13 |
| S14 | Import and duplicate handling | M1-P0 | R14 |
| S15 | Search and command overlay | M1-P1 | R15 |
| S16 | Members and effective access | M1-P0 | R16 |
| S17 | Invitations and teammate onboarding | M1-P0 | R17 |
| S18 | Restricted resource and access requests | M1-P0 | R18 |
| S19 | Share, audience and data preview | M1-P0 | R19 |
| S20 | Comments, mentions and thread context | M1-P0 | R20 |
| S21 | Cloud fork proposal and review diff | M1-P0 | R21 |
| S22 | Cloud conflict resolution and merge | M1-P0 | R22 |
| S23 | Notifications and team activity | M1-P0 | R23 |
| S24 | Local/Git and cloud state | M1-P0 | R24 |
| S25 | Offline, access loss and draft recovery | M1-P0 | R25 |
| S26 | Preferences, density and accessibility | M1-P1 | R26 |
| S27 | Protocol and automation extension map | M2-map | R27 |
| S28 | Organization and ecosystem extension map | M3-map | R28 |

## S01 · Workspace overview and switcher

**Stage:** M1-P0 · **Requirement:** R01

**Purpose:** Orient a returning developer or a newly joined teammate in Orbit Labs / Commerce API.

**Data and content:** Workspace purpose, visibility, effective access, shared collections/environments, first useful request, current location, recent changes.

**Required states:** Workspace ready; Empty workspace; Restricted workspace.

**Acceptance:** A returning user can resume work; Sam can find List orders and credential setup without being shown administrative controls. No-access results reveal only permitted metadata.

## S02 · Workbench, collection tree and tabs

**Stage:** M1-P0 · **Requirement:** R02

**Purpose:** Move between requests while preserving editing context.

**Data and content:** Collection/folder/request hierarchy; request tabs; active selection; dirty/save state; environment; resizable request/response split.

**Required states:** Clean saved tab; Dirty request tab; Close dirty tab; Save failed.

**Acceptance:** Closing a dirty tab offers a clear keep/save/discard route. A failed save retains the draft and never changes its badge to Saved. Personal panel positions are not shared with teammates.

## S03 · HTTP composer and data editors

**Stage:** M1-P0 · **Requirement:** R03

**Purpose:** Specify the exact HTTP request before sending.

**Data and content:** Method, URL, query rows, headers, body mode and body, enabled rows, generated auth headers and edit source.

**Required states:** Repeated query keys; Invalid JSON body; Disabled parameter/header; Generated authorization header.

**Acceptance:** Repeated query keys survive round trips between URL and table. Generated auth headers lead to the auth source. Validation never silently rewrites the intended payload.

## S04 · Send and execution outcome

**Stage:** M1-P0 · **Requirement:** R04

**Purpose:** Run a request and show what happened without conflating transport, HTTP status and assertions.

**Data and content:** Execution ID; method/endpoint snapshot; environment identity; start/end time; HTTP status when present; transport error; cancellation state.

**Required states:** No execution yet; Sending; HTTP 200 received; HTTP 401 received; Transport failure; Client canceled.

**Acceptance:** A DNS failure has no invented HTTP status. A 401 retains its server body. Cancellation says the client stopped waiting and does not claim the server rolled back a mutation.

## S05 · Response inspection and freshness

**Stage:** M1-P0 · **Requirement:** R05

**Purpose:** Inspect body, headers, cookies, timing and size from one concrete execution.

**Data and content:** Execution snapshot, formatted/raw content, body type, search/filter, response metadata, test link and current-editor mismatch.

**Required states:** 204 No Content; Large response; Binary response; Filter has no matches; Editor differs from response.

**Acceptance:** Editing the URL, environment or auth after a send does not rewrite the old response. The viewer identifies the earlier execution and offers an explicit rerun; 204 displays No content.

## S06 · Authorization and inheritance

**Stage:** M1-P0 · **Requirement:** R06

**Purpose:** Identify and change the effective auth source without exposing credentials.

**Data and content:** Inherited parent and auth type; request override; no-auth state; secure variable/vault reference; generated fields; availability.

**Required states:** Inherited authorization; Request override; Missing credential reference.

**Acceptance:** Users can navigate to parent auth or choose a request override. Workspace membership is never presented as API authorization. Fake fixture credentials remain masked by default.

## S07 · Environment selection and variable resolution

**Stage:** M1-P0 · **Requirement:** R07

**Purpose:** Explain where the next request will go and which value wins.

**Data and content:** Active environment; resolution source; unresolved token; shadowed alternatives; effective endpoint; local/shared value context.

**Required states:** Environment changed after send; Unresolved variable; Shadowed variable; No active environment.

**Acceptance:** Changing environment never sends a request automatically. Scope precedence is preserved. The previous response retains its old environment identity and is marked as earlier context.

## S08 · Environment editor and secret references

**Stage:** M1-P0 · **Requirement:** R08

**Purpose:** Manage shared configuration and each user's personal credential separately.

**Data and content:** Variable definitions; local/shared values; scope; secure reference; secret access; masked value; source and role.

**Required states:** Viewer edits local value; Editor changes shared value; Shared secret inaccessible; Vault locked/unavailable.

**Acceptance:** Sam may edit their permitted personal local value but cannot change the shared definition. Sharing a secure variable and granting access to its referenced shared secret remain distinct.

## S09 · Scripts and individual test results

**Stage:** M1-P0 · **Requirement:** R09

**Purpose:** Write pre-request/post-response logic and inspect assertions.

**Data and content:** Script phase; collection/folder/request origin; editor; runtime exception; pass/fail/skip rows; associated execution.

**Required states:** Assertions passed; 200 with failed assertion; Expected 401 test passed; Script exception.

**Acceptance:** Show 200 plus failed tests and expected 401 plus passed negative checks. A pre-request failure is distinct from an HTTP error. Script hierarchy is collection → folder → request for both phases.

## S10 · Collection overview and organization

**Stage:** M1-P1 · **Requirement:** R10

**Purpose:** Understand and organize reusable API work.

**Data and content:** Commerce API description; requests/folders; inherited settings; examples; resource access; save and revision context.

**Required states:** Collection ready; Collection read only.

**Acceptance:** Read-only users can discover/run accessible requests and use permitted copy/fork paths. A duplicate is identified as a new artifact; a duplicate tab does not imply a new saved request.

## S11 · Collection Runner setup

**Stage:** M1-P1 · **Requirement:** R11

**Purpose:** Prepare a bounded functional HTTP run.

**Data and content:** Selected requests/order; environment; iterations; data preview; delay; persistence options; execution target.

**Required states:** Run configured; Invalid iteration data; No requests selected.

**Acceptance:** The start action identifies the chosen environment, count and dataset. Invalid data blocks a misleading run start and preserves the selection for correction.

## S12 · Collection run progress and results

**Stage:** M1-P1 · **Requirement:** R12

**Purpose:** Find the failed request/iteration and understand the evidence.

**Data and content:** Run ID; request revision if recorded; environment; iteration; execution and assertion outcomes; skipped/canceled counts; payload retention.

**Required states:** Run in progress; Assertion failure drilldown; Canceled and skipped; Payload not retained.

**Acceptance:** A canceled run is not marked passed. Missing request/response payloads do not erase summarized results or imply that the payload can be shared. Test evidence stays tied to its recorded context.

## S13 · History and response snapshots

**Stage:** M1-P1 · **Requirement:** R13

**Purpose:** Reuse prior requests without confusing historical results with the live editor.

**Data and content:** Timestamp; request snapshot; environment identity; response availability; retained metadata; reopen/restore actions.

**Required states:** Historical snapshot available; Historical response unavailable; Restore while current draft dirty.

**Acceptance:** Opening history creates a separate historical view or tab. Restoring request settings requires an explicit action and does not overwrite a dirty current draft. An old response is never relabeled as a new run.

## S14 · Import and duplicate handling

**Stage:** M1-P0 · **Requirement:** R14

**Purpose:** Turn cURL or supported native/API files into inspectable requests.

**Data and content:** Detected format; parsing result; destination; request/resource preview; conflicts; warnings; import summary.

**Required states:** Import preview ready; Import parsing failed; Import duplicate detected.

**Acceptance:** Malformed input has a useful error location. Duplicate names/IDs are resolved before import and unrelated work is not overwritten silently. Initial fixtures use HTTP imports.

## S15 · Search and command overlay

**Stage:** M1-P1 · **Requirement:** R15

**Purpose:** Find requests and navigate to actions without losing the workbench.

**Data and content:** Accessible results; workspace/collection scope; type; recent requests; keyboard selection; empty/error status.

**Required states:** Accessible search results; No search results.

**Acceptance:** Search does not leak inaccessible content. Opening a result focuses its artifact; executing an action requiring review opens its flow rather than performing a hidden side effect.

## S16 · Members and effective access

**Stage:** M1-P0 · **Requirement:** R16

**Purpose:** See who has access and let authorized users manage it.

**Data and content:** Alex, Mina, Sam and Jo; current membership; workspace role; resource grants; effective capabilities; pending changes.

**Required states:** Members loaded; Role change preview; Permission changed during dialog.

**Acceptance:** Role labels and effective permissions are separate. Jo sees a before/after access preview; revoked capability while editing the dialog blocks the action without claiming success.

## S17 · Invitations and teammate onboarding

**Stage:** M1-P0 · **Requirement:** R17

**Purpose:** Bring Sam into the intended workspace and to a first successful request.

**Data and content:** Invite target; workspace; role description; invitation status; account identity; onboarding checklist; credential setup.

**Required states:** Invalid invite target; Invitation pending; Invitation accepted; Invitation expired; Invitation revoked; Invitation send failed; Account mismatch.

**Acceptance:** Pending is distinct from accepted. A failed invite retains input. Account mismatch cannot silently accept for the wrong person. The prototype simulates sending and never contacts real recipients.

## S18 · Restricted resource and access requests

**Stage:** M1-P0 · **Requirement:** R18

**Purpose:** Explain unavailable actions and request the right scope of access.

**Data and content:** Workspace/resource identity where permitted; missing capability; current access; request target; request status; approver route.

**Required states:** Access required; Access request pending; Access granted; Access denied or expired.

**Acceptance:** A request asks for a specific capability on a specific resource. A pending request does not unlock editing. Approval refreshes effective capabilities before enabling the action.

## S19 · Share, audience and data preview

**Stage:** M1-P0 · **Requirement:** R19

**Purpose:** Make the recipient's access and included data reviewable.

**Data and content:** Resource/version; recipients or audience; roles; link mode; included environment; shared values; excluded local values; response/run snapshot preview.

**Required states:** Private link copied; Share preview excludes local data; Broader audience preview; Secret reference without recipient access.

**Acceptance:** Copying an ordinary link does not grant access. Excluded data is not transmitted; masking alone is not described as privacy. Broader visibility identifies the changed audience before applying.

## S20 · Comments, mentions and thread context

**Stage:** M1-P0 · **Requirement:** R20

**Purpose:** Discuss a concrete request, field or diff with teammates.

**Data and content:** Thread ID; anchor; author; timestamp; revision if known; replies; draft; mention/access state; resolved flag.

**Required states:** No comments; Comment posting; Comment posted; Comment post failed; Thread resolved; Thread reopened; Original context changed; Mention lacks access.

**Acceptance:** Posting failure retains the draft and retry avoids duplicates. A mention does not grant access. Changed anchors show Original context changed rather than attaching the thread to unrelated text.

## S21 · Cloud fork proposal and review diff

**Stage:** M1-P0 · **Requirement:** R21

**Purpose:** Let Mina review Alex's HTTP collection change and its evidence.

**Data and content:** Postman parent/fork; source/target workspace; compared revisions; changed-object tree; diff; review assignment; comments; run evidence.

**Required states:** Fork proposal draft; Review open; Feedback requests correction; Approved but not merged; Evidence stale or unrecorded; Merge unavailable to viewer.

**Acceptance:** Review assignment is not a global role. Approval is distinct from merge. Changed source or unrecorded revision cannot keep a falsely current green-evidence badge. Changes requested is a proposed status unless implemented.

## S22 · Cloud conflict resolution and merge

**Stage:** M1-P0 · **Requirement:** R22

**Purpose:** Resolve a Postman element conflict without discarding either side invisibly.

**Data and content:** Source/target versions; base if actually available; field conflicts; resolution choices; resulting content; authorization; target revision.

**Required states:** Unresolved cloud conflict; Resolution preview; Invalid combined content; Target advanced during resolution; Merge failed or permission lost; Merge succeeded.

**Acceptance:** The base value limit=20, fork/source value 50, and advanced target value 100 are visible in context. Merge requires all conflicts resolved, valid content, current target and effective merge permission.

## S23 · Notifications and team activity

**Stage:** M1-P0 · **Requirement:** R23

**Purpose:** Return teammates to actionable API work.

**Data and content:** Actor; action; object; timestamp; mention/review/access event; read state; target/revision access.

**Required states:** Unread actionable event; Notification target unavailable.

**Acceptance:** Unread is not unresolved. Opening an event reaches its actual thread/request; unavailable targets show an access or deletion state without exposing private content. Failed actions are not success activity.

## S24 · Local/Git and cloud state

**Stage:** M1-P0 · **Requirement:** R24

**Purpose:** Explain where changes live and how they become team-visible.

**Data and content:** Local artifact; repository/branch/commit when available; remote status; cloud resource/version; pending push/pull; comment availability.

**Required states:** Local-only artifact; Committed but not pushed; Git remote updated; cloud outdated; Cloud artifact ready.

**Acceptance:** Local save, Git commit, Git push and Postman cloud sync are separate states. Local-only resources do not pretend to have cloud comments. Git provider review remains a separate route from Postman fork review.

## S25 · Offline, access loss and draft recovery

**Stage:** M1-P0 · **Requirement:** R25

**Purpose:** Preserve permitted work while describing unavailable operations honestly.

**Data and content:** Collaboration connectivity; target API connectivity; dirty local draft; current permissions; reconnect comparison; queued action IDs.

**Required states:** Collaboration offline; local API reachable; Reconnect with conflicting edits; Edit access revoked with dirty draft; Recover local draft after interruption.

**Acceptance:** A local API can remain reachable when collaboration is offline. Permission loss blocks shared writes while retaining permitted recovery; reconnect checks versions before replaying writes and avoids duplicate comments.

## S26 · Preferences, density and accessibility

**Stage:** M1-P1 · **Requirement:** R26

**Purpose:** Keep the workbench usable at target sizes and with keyboard/zoom.

**Data and content:** Theme, density, split orientation, personal panel widths, focus state, reduced-motion behavior and labels.

**Required states:** Narrow or zoomed layout; Keyboard and focus path.

**Acceptance:** At 1280×800 collapse secondary panels before squeezing code. Keyboard focus and actions remain reachable at 200% zoom; unified diff replaces illegibly narrow side-by-side columns.

## S27 · Protocol and automation extension map

**Stage:** M2-map · **Requirement:** R27

**Purpose:** Reserve and specify later protocol-specific and automation workflows.

**Data and content:** GraphQL/gRPC/streaming session models; schema/messages; mocks/monitors/docs/specs; current capability inventory.

**Required states:** Later protocol/automation inventory.

**Acceptance:** This is a coverage map only. Do not claim detailed feature parity or reuse HTTP Send/response states blindly for streaming/session protocols. Verify each capability before adding real controls.

## S28 · Organization and ecosystem extension map

**Stage:** M3-map · **Requirement:** R28

**Purpose:** Track remaining platform scope without inventing entitlements.

**Data and content:** Public/partner workspaces; governance; integrations; audit; billing/limits; advanced AI; org-level authority.

**Required states:** Later organization/ecosystem inventory.

**Acceptance:** Mark each capability retained/redesigned/deferred/removed and why. Prototype a plan-specific operation only after its permission, entitlement and audience behavior are verified.

## Golden-path coverage

| Flow | Screen coverage | Evidence to keep |
| --- | --- | --- |
| First useful request | S01 → S14 → S02/S03 → S07/S06/S08 → S04/S05/S09 | Fixture execution, request/environment snapshot, error recovery, save result. |
| Team handoff and review | S15/S16/S17/S18 → S19 → S20/S21 → S22/S23 → S08/S04 | Invite and access scope; recipient local credential; compared fork/parent revisions; comments; run context; merge result. |
| Concurrent/offline recovery | S02/S20/S21 → S24/S25 → S22 | Preserved draft/comment; connectivity distinctions; authorization refresh; resolution and resulting revision. |

An arrow is a task progression, not a requirement to make each family a full-page step. Use direct links and contextual panels where appropriate.

## Acceptance evidence and handoff

For each R01–R26, record the Figma frame/component IDs, prototype route, covered CSV states, reviewer result and open questions. Demonstrate both an allowed and a restricted team action. The invitation, sharing, comment, access-change and merge simulations must be clearly identifiable in the prototype.

Use the same fixture values throughout: List orders at rev18, the four-check success result, the expired-auth scenario, status=processing, and proposal-42 changing limit from 20 to 50 at rev19. The conflict variant advances the target to rev20 with limit=100; the base is 20, source is 50, and target is 100. Mina's supplied comment asks to keep the pagination check. Do not invent timing, response size or IDs if those fields already exist in the fixture; use its exact values. Missing metadata stays unknown.

The supplied scenarios seed the golden paths and selected edge cases; the CSV is the broader behavior contract. Before implementing a matrix state without a matching scenario, add a named deterministic fixture for it. In particular, distinguish the supplied offline-cloud scenario's unknown API availability from a separate test in which a local API is confirmed reachable.

Do not count a static disabled button as an authorization implementation or a passing test badge as execution evidence. Design validation demonstrates the interaction; production validation still needs the real API, permissions, persistence and synchronization contracts.

## Verified constraints and current caveats

These constraints come from the official documentation read on 17 November 2026; the layouts and recovery improvements above are proposals.

- The retrieved default docs identify Latest (v12). Documentation screenshots may depict older UI. Pin the actual app build and plan before judging parity.
- Variable scope is distinct from local/shared value. Global → collection → environment → data → local is broad-to-narrow scope precedence. [Variable documentation](https://learning.postman.com/docs/use/send-requests/variables/variables/)
- Environment Viewers can edit their own local value, while shared-value editing requires the relevant permission. [Team environments](https://learning.postman.com/docs/use/send-requests/variables/team-environments)
- Shared Vault storage, sharing a secure variable, and the secret's access scope are separate. Direct `{{vault:name}}` references are documented for Local Vault; shared secrets use a secure variable. [Vault use](https://learning.postman.com/docs/use/postman-vault/use-vault-secrets)
- Comments are unavailable in local mode for Git-connected workspaces until changes are synced to Cloud. @mentions still require resource access. Revision-aware comment anchors in this redesign are a proposal requiring backend support. [Comments](https://learning.postman.com/docs/collaborating-in-postman/comments/)
- This first cloud review fixture is HTTP-only. Current documentation limits pull/merge/conflict operations for collections containing non-HTTP requests. Postman element forks/PRs and Git-provider branches/PRs remain different models. [Forks](https://learning.postman.com/docs/collaborating-in-postman/using-version-control/forking-elements/) · [Native Git](https://learning.postman.com/docs/use/native-git/overview)
- Merging a Postman fork requires target Editor access; updating/deleting its source also requires source access. Direct merge may be available without a PR. A mandatory approval gate or formal Changes requested status in this proposal must not be represented as an existing universal rule. [Review and merge](https://learning.postman.com/docs/collaborating-in-postman/using-version-control/reviewing-pull-requests)
- Permission composition and entitlement must be verified for the target build. **Resource-move guidance differs between official pages:** the environment guide permits an environment Editor or workspace Admin, while workspace management states workspace Admin or team Super Admin. Do not hard-code an unconditional move permission from this pack. [Workspace management](https://learning.postman.com/docs/collaborating-in-postman/using-workspaces/internal-workspaces/manage-workspaces/) · [Roles](https://learning.postman.com/docs/administration/roles-and-permissions)
- History and run summaries do not guarantee retention of response bodies. Sharing can include only the available, authorized data. [Navigation/history](https://learning.postman.com/docs/getting-started/basics/navigating-postman/) · [Collection Runner](https://learning.postman.com/docs/tests-and-scripts/running-collections/intro-to-collection-runs/)
- A desktop-only workbench does not need the web app's request-agent selector. Browser-agent/CORS/private-network behavior needs a separate browser adaptation. [Request agents](https://learning.postman.com/docs/getting-started/basics/about-postman-agent/)

