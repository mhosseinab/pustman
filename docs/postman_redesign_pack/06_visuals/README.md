# Original layout blueprints

These are original proposed starting layouts, not Postman screenshots and not finished Figma designs. The app region in each is exactly 1440 × 900; the total image is 1520 × 1060 to include the title and provenance caption.

- `workbench_blueprint.svg` / `.png`: request and response remain the primary task; team comments stay accessible in context. The response shows the first 14 of 29 lines of the canonical `http_200` fixture, with a scrollbar and visible line-count label. It is not a shortened substitute body. The complete fixture contains two orders with customer objects, `page.limit: 20`, `page.next_cursor: "cursor_demo_02"`, and `request_id: "req_demo_77"`. The collection tree follows the six supplied collection requests.
- `collaboration_review_blueprint.svg` / `.png`: a proposed cloud-fork review showing parent rev18 versus proposal rev19, a 20 → 50 query-parameter change, revision-specific validation and discussion, and separate approval/merge actions. The status=processing parameter is unchanged.

All values and identities are synthetic. The SVGs contain editable shapes and text; they are suitable for visual reference or importing as a starting composition. Importing an SVG does not produce a complete Figma component system, auto-layout structure, variable bindings, or working interactions. Build those explicitly using the prompt pack.

The dark colors match the proposed design tokens. For reproducible PNG rendering, the blueprint uses installed Open Sans and DejaVu Sans Mono as substitutes for the spec’s proposed Inter and JetBrains Mono.

Review note: the approval gate is a proposed demo policy, not a guaranteed product-wide Postman rule. Collaboration enhancements and review behavior are design proposals; no live co-editing or backend authorization is implemented by these assets.
