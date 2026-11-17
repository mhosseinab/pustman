# Verification status

Prepared 17 November 2026. This records checks on the resource pack, not proof that the proposed redesign improves usability.

## Completed

- Researched product and tooling behavior using official Postman, Figma and W3C sources. The source register has 45 links, with evidence limits and unresolved questions retained.
- Retrieved 16 official documentation image originals. File hashes and dimensions match the provenance manifest. Eleven image paths are labeled v12; five are legacy v10/v11 assets. The exact app build and original capture date are unknown for each image.
- Checked all JSON files parse and the state matrix contains 28 screen families and 106 state rows. Twenty-six screen families specify the first milestone; the final two are maps for later coverage.
- Validated the six-request collection against the official Postman Collection v2.1 JSON schema: https://schema.getpostman.com/json/collection/v2.1.0/collection.json
- Checked 22 intended foreground/background token combinations against the documented contrast targets. These checks do not establish accessibility of an implemented interface.
- Reviewed consistency across the sample data, prompt sequence, screen specifications and two original layout blueprints. Corrected invitation state, test-profile and resource-name mismatches.
- Rendered and visually inspected the PDF playbook and reference atlas. Inspected the original SVG layouts through their PNG renders.

## Requires a live project

- No native Figma file, component library or functioning prototype has been created. The SVG blueprints are editable vector sources; they do not substitute for native Figma variables, components and auto-layout.
- Current account eligibility, Figma capabilities and the user's installed Postman build have not been checked. Use prompt 01 and the capture checklist before treating old documentation images as the target baseline.
- No user interviews, task tests, measured speed improvements or accessibility audit have been performed. The testing plan and performance targets are proposed inputs.
- The fixtures use fictional people and non-live `.example` API endpoints. HTTP results, permissions, invitations and collaboration outcomes are deterministic demonstration data, not observed server behavior.
- The supplied Postman collection has not been imported or executed in Postman. The environment and OpenAPI example have not been validated by a running application.
- Authentication, backend permission composition, synchronization, concurrent changes and protocol-specific behavior still need implementation and integration validation.
- Complete Postman feature parity has not been specified. The pack provides a substantial first milestone and explicit maps for later protocol, automation, organization and ecosystem work.

## Decisions to preserve

Keep proposed behavior separate from documented behavior. In particular, the example approval gate, comment revision anchoring, demo capabilities and UI labels are project proposals. Resolve the environment-move permission discrepancy noted in the source register against the actual target build and plan before implementing that operation.
