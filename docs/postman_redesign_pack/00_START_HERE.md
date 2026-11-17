# Postman redesign resource pack

Prepared 17 November 2026

## Recommended starting point

Use the Figma agent inside Figma Design to build an editable system and screens. Treat the request/debugging flow and the team handoff/review flow as equally important. Add Code Layers or Figma Make when behavior needs to run. Keep the design frames and runnable prototype explicitly synchronized.

This pack is ready to use as project input. It includes proposed specifications and original layout blueprints, not a completed native Figma project. The official screenshots are evidence of existing UI; the blueprints are proposals. No user interviews or live app audit were performed.

## First session

1. Create a new Figma Design file named `Postman redesign - API workbench and teams`.
2. Open Agents. Attach `01_specs/product_brief.md`, `01_specs/collaboration_spec.md` and `01_specs/design_system.md`.
3. Attach the relevant screenshots rather than the entire archive. Begin with 01 (request), 02 (application overview), 09 (invite), 11 (comments) and 13 (review). Image 13 is a legacy v11 reference.
4. Paste `02_prompts/00_operating_contract.md`, then run prompt 01. Inspect its assumptions and editable-component check.
5. Run prompt 02 to explore the same core task in three layouts. Pick a direction based on the execution and collaboration tasks.
6. Run prompts 03 and 04 for foundations and reusable components, then prompt 05 for the first complete workbench.
7. Build the remaining paths with prompts 06-10, test modes and resizing with 11, then move approved frames into a functional prototype with 12.
8. Review with prompt 13 and the validation scripts; produce handoff with 14. Prompt 15 is the optional external-agent/MCP route.

If Figma Specs is enabled, keep the brief, state contract and decisions beside the design in a native Spec. Otherwise attach the Markdown sources as context. Availability depends on current account/file access.

## Pack map

| Location | Purpose |
| --- | --- |
| 01_specs/product_brief.md | Audience, scope, two golden paths, design hypotheses, success measures |
| 01_specs/screen_specs.md | Screen-family behavior and acceptance contracts |
| 01_specs/screen_state_matrix.csv | States, triggers, visible feedback, actions, recovery and permissions |
| 01_specs/collaboration_spec.md | Membership, sharing, comments, review, conflicts, environments, access loss |
| 01_specs/design_system.md and design_tokens.json | Native Figma structure, components, sizes, themes and token seed |
| 02_prompts/ | 16 ordered, copy-paste prompts with scope and review gates |
| 03_references/ | 16 official screenshot originals, provenance, current-build capture checklist and source register |
| 04_fixtures/ | Synthetic API responses, collaboration scenarios, explicit demo permissions, Postman collection/environment and OpenAPI example |
| 05_validation/ | Paired-user tasks, design review checklist, decision log, handoff template, contrast report |
| 06_visuals/ | Original workbench and team-review layout blueprints, clearly labeled as proposals |
| 07_guides/ | Readable PDF playbook and visual reference atlas |

See `05_validation/verification_status.md` for completed checks and work that still requires a live product, Figma file or user study.

## Figma roles in this workflow

- Design agent: native components, variables, layout and visual iteration.
- External agent plus official remote MCP: optional repeatable batch changes and structure inspection. Use the official Figma plugin/skills for your supported client; the endpoint alone does not provide all workflow guidance.
- Make or Code Layer: executable prototype with deterministic fixture data. Native Design-agent prototype wiring is not assumed; check the current support table or wire native interactions manually.
- Specs: living requirements and decisions when enabled.
- Dev Mode and Code Connect: later implementation handoff, especially after real reusable code components exist.

See the verified setup links F1, F2, F5, F6, F9 and F10 in `03_references/source_register.md`. Older First Draft tutorials do not describe the recommended current entry point. Copying a Make preview into Design does not create automatic bidirectional synchronization.

## Working rules

Use one foundation owner if multiple agents work in parallel. Assign other agents separate pages or screen families. Batch a few related screens at a time; require visual inspection and structural readback after each batch. Keep an explicit decision log when scope or behavior changes.

Do not turn every state into a full screen. Use component variants for small state changes and separate frames when the overall flow or layout changes. Keep the collection/editor/response loop compact; comments can be contextual, while a review diff deserves the full content area.

Use fake data throughout. The included `.example` endpoints are deliberately non-live. The collection can be imported to inspect structure and saved response examples; it does not provide a running API. Send behavior, invitations, permissions and collaboration in a prototype must use the included simulation fixtures.

## Scope boundary

This is a substantial first-milestone specification plus a map of later surfaces. It does not claim complete Postman feature parity. Preserve the backlog for protocol-specific clients, automation, organization administration, public/partner workflows and other product areas. Expand those after both core golden paths are usable.
