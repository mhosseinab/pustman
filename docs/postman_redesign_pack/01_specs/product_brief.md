# Postman redesign: product brief

Version 1.0 · Prepared 17 November 2026 · Design proposal, not a description of a shipped Postman redesign.

## Decision

Design a focused API workbench with team collaboration built into the same workflow. Keep the request, response, execution context, and review context close together. Use Figma Design as the editable source of truth; validate behavior in a separate code layer or Figma Make prototype.

The initial design milestone covers two complete paths: a developer gets a request working, and a teammate safely reviews, reuses, and changes that work. A simpler individual API client alone would not meet this brief.

## What is known, assumed, and proposed

- Known: the user wants a from-scratch Postman redesign using agentic Figma and explicitly includes team collaboration.
- Assumed for this starting pack: desktop first; experienced backend developers and small-to-medium engineering teams; REST/HTTP first; both light and dark modes; internal team use before public and partner use.
- Not known: the user's exact Postman build, operating system for this project, Figma seat, team size, enterprise entitlements, current pain points, brand constraints, and whether implementation is planned.
- Proposed: the navigation, visual system, state layouts, approval presentation, task metrics, and prioritization in this pack. These are design hypotheses to test.
- Evidence baseline: current official Postman documentation is labeled Latest (v12). A documentation image may still depict an older interface. Use each image's provenance entry; do not infer the installed build from the page date.

This pack contains research inputs, proposed specs, agent prompts, fixtures, reference screenshots, and original layout blueprints. It is not a completed native Figma file or a tested implementation.

## People and jobs

| Person | Job | What the design must make clear |
| --- | --- | --- |
| Alex, backend engineer | Build and debug requests; update shared collections | Active environment, resolved variables, inherited auth, response origin, unsaved changes |
| Mina, engineer reviewing changes | Understand a change and judge its evidence | Before/after, author, request snapshot, test run and environment, unresolved conflicts |
| Sam, teammate with view access | Discover and run a known example | What can be executed or copied, what cannot be edited, and how to request access |
| Jo, workspace admin | Onboard people and control access | Audience, effective permission, pending invitation, safe change previews |

All names, companies, addresses and payloads in fixtures are fictional.

## Product principles

1. Make the next action obvious without hiding the execution context. The URL, method, environment and Send action should remain easy to find.
2. Treat collaboration as work on an API artifact: who changed it, why, against which version, with what test evidence, and who can act next.
3. Distinguish local data, shared data, and secret references. A saved value is not automatically a shared value.
4. Preserve technical semantics. A 401 response, a TLS failure and a test failure are different outcomes. A Git branch and a Postman fork are different versioning models.
5. Support experts with density, resizable panels and keyboard access. Keep labels and reliable targets; density must not mean unreadably small text.
6. Make state visible. Display unsaved, local-only, publishing, synced, offline, conflict, read-only and access-lost states where they affect the current artifact.
7. Keep AI contextual. Suggestions may help explain an error or draft a test, but the user must be able to inspect and apply changes.

## Scope and delivery order

### Milestone 1: individual execution plus team reuse

Include workspace switcher, collection tree, HTTP request builder, auth, environments, variable inspector, response viewer, console diagnostics, scripts/test results, import, history/search, collection overview, runner, workspace overview, members/access, sharing, comments, change review, conflict recovery, teammate onboarding and notifications. Include local/Git versus cloud context and offline/access-revoked recovery.

Design these as complete flows before expanding the visual treatment to every product surface. This is a sequence of work, not a decision to remove Postman's remaining capabilities.

### Milestone 2: protocol-specific work and automation

Extend the shell for GraphQL, gRPC, WebSocket and other currently supported protocols after a current capability inventory. Design connection/session state, message history, schema and subscription behavior specifically for each protocol. Add monitors, mocks, documentation, API definitions and automation entry points with their own states.

### Milestone 3: organization and ecosystem

Cover enterprise administration, public/partner workspaces, governance, integrations, detailed audit history, billing/limits, advanced AI/agent workflows and other currently available product areas. The inventory must record what is retained, redesigned, deferred or deliberately removed, with a reason. Do not silently drop a feature because it did not fit the main screen.

## Three directions to explore

Use the same fixture and tasks for each direction so that the comparison measures layout choices.

| Direction | Layout | Strength to test | Risk to test |
| --- | --- | --- | --- |
| A: focused workbench, recommended starting point | Collection tree; request over response; optional collaboration panel | Easy scanning of execution context and response | A vertical split may leave too little editor height |
| B: side-by-side inspector | Collection tree; request left and response right; review separate | Comparing request and response | Narrow laptops and long JSON lines |
| C: team project hub | Workspace overview and reviews lead into a workbench | Discoverability and onboarding | Extra navigation for frequent execution |

Explore these at low fidelity. Choose using observed tasks and comments, not a preference vote on screenshots.

## Primary navigation proposal

Global: team/workspace switcher, universal search, activity inbox, help/settings.

Workspace: Collections, Environments, Runs, Reviews, Members. Put secondary product areas in a labeled overflow or workspace directory with search. Keep collection context within the workbench; avoid a separate full-width navigation bar for every hierarchy level.

Workbench: collection tree; request tabs; breadcrumb and artifact state; method/URL/Send; configuration tabs; response; optional contextual panel. Tabs, split orientation, open sidebar and panel widths are personal UI preferences. Sharing a collection must not force another person's panel layout.

Collaboration: comments and activity use a contextual panel; review/diff and people/permissions use full-width views because they need more space. An avatar indicates identity or ownership unless reliable presence is actually implemented. Do not imply simultaneous co-editing by decoration.

## Two golden paths

### Flow A: first useful request

1. Open a workspace and import the supplied collection or API description.
2. Choose Development; inspect how base_url and access_token resolve.
3. Send List orders; inspect a 200 response and four passing checks.
4. Change the scenario to expired auth; distinguish the 401 response from test outcomes.
5. Follow the auth source, fix the local credential reference, rerun and save the request.

### Flow B: successful team handoff

1. Jo invites Sam with a clearly described role; Sam sees pending/accepted states and joins the correct workspace.
2. Alex proposes increasing the page size on a forked cloud collection. The proposal identifies its parent and versions.
3. Alex attaches a run summary with request revision and environment name; local secret values are excluded.
4. Mina reviews the diff, leaves an inline comment and requests a change. Alex updates the proposal and reruns the checks.
5. A parent change creates a conflict; the resolver explains both versions and the resulting value.
6. An authorized person merges; Sam sees the updated shared artifact, runs it with their own local credential, and cannot overwrite the shared source without permission.

Use Git-native work as a separate route with commit, branch and remote terms. Do not relabel a cloud fork as a Git branch or imply all Git pull requests live inside Postman.

## Success measures: targets to validate, not existing performance claims

Capture a baseline on the current product with the same users, scenarios and fixtures. Measure time, task completion, context mistakes and confidence. Suggested acceptance targets:

- A returning user can find a named request, verify the environment and send it without help.
- A new teammate can distinguish workspace access from API authorization and explain why they cannot edit a resource.
- A reviewer can identify the exact request revision and environment behind a run result.
- No critical usability failure causes unintended sharing of local values, silent overwrite of a conflict, or confusion about the destination environment in the pilot.
- Target a 20% reduction in median time on repeated core tasks relative to the measured baseline, with no regression in team handoff accuracy. This is a design target, not a predicted improvement.
- Every critical flow works through the keyboard in the runnable prototype; focus returns to the triggering control after dialogs close.

Use an initial 5-8 participant formative study, including at least two pairs who complete the handoff together. This is a practical recruitment starting point, not a claim of statistical sufficiency. Follow up when findings differ by expertise, accessibility needs or team role.

## Review gates

G0: current-state evidence, scope and assumptions are recorded.
G1: an information architecture and two complete task flows are accepted.
G2: foundations and components exist as editable native elements; one workbench frame resizes correctly.
G3: both golden paths include error, empty, loading, read-only and conflict states.
G4: runnable behavior matches the state matrix and fixtures; accessibility and paired-team tasks are evaluated.
G5: engineering handoff includes source links, component inventory, tokens, state transitions, open questions and evidence.

Advance one gate at a time. Keep changes small enough to inspect and undo. A beautiful success screen is not evidence that a workflow is complete.

## Decisions to revisit before implementation

Confirm the target build and platform; cloud versus local/Git emphasis; account entitlements; workspace and resource permission composition; approval policy versus actual merge authority; comment anchoring; allowed sharing of response bodies and run data; storage/sync behavior; protocol coverage; and implementation technology. The design can progress with these marked assumptions, but it must not invent backend guarantees.

## Sources

Product semantics are indexed in ../03_references/source_register.md and the research notes. Original proposals and numeric design targets in this document are authored for this project and are not represented as research findings.
