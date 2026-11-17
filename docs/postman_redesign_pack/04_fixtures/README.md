# Fixture guide

Everything here is synthetic. No API server is included, the `.example` hosts are deliberately non-live, and no invitation or message should be sent to the listed addresses.

## Files

- `scenarios.json`: deterministic request outcomes and collaboration edge cases. Select by scenario ID instead of using random responses.
- `team_permissions.json`: Jo, Alex, Mina, Sam and a person without access. Capabilities are explicit demo values, not a universal Postman RBAC engine.
- `variable_resolution.json`: scope precedence, local/shared values, masking, and separate Local/Shared Vault reference examples.
- `review_fixture.json`: a cloud HTTP collection fork, run evidence, review assignment, comments and a conflict variant.
- `commerce_api.postman_collection.json`: a collection with six HTTP requests and saved response examples, validated against the official Postman Collection v2.1 JSON schema. Import for UI reference and offline example inspection. It has not been imported or executed in a running Postman app.
- `development.postman_environment.json`: non-live base URL and blank token. No credential is supplied. Importing it does not establish private local/shared values; configure those in the actual app when capturing a baseline.
- `post_response_tests.js`: four example assertions for List orders.
- `openapi.yaml`: a small synthetic OpenAPI 3.1 contract. It is project input, not a real service specification.
- `import_example.curl.txt`: a deliberately non-live cURL example with a conspicuously fake token.

## Scenario interpretation

Never conflate the transport result, HTTP response and assertions. A transport failure has no HTTP status. A 200 response may have a failed assertion. An intentional 401 may pass a negative test. A 204 is a completed response with no body.

The default workbench is List orders at `rev18` with `status=processing&limit=20`. The review fixture compares that parent against proposed `rev19`, which changes only limit from 20 to 50 and preserves status=processing. Review/test snapshots must identify the relevant revision. The original workbench and review blueprints deliberately illustrate different stages of the work.

The review policy requiring approval is a proposed project setting. Do not infer that every Postman merge requires it. The fixtures assume effective parent Editor access for merging; an assigned reviewer alone does not gain that permission.

Each canned HTTP outcome identifies its test profile. The normal List orders profile runs four independent checks; malformed data can fail multiple checks. The expected-401 and 204 examples use separate scripts and demo request revisions, not the unchanged List orders script. Results are deterministic fixtures, not measurements from a live API.

The baseline member list represents accepted memberships. For onboarding, apply invitationVariants.pending, which removes Sam's workspace/resource access until acceptance; do not show the baseline capabilities while the invitation is pending. Thread resolution and reopening use explicit own-thread versus other-thread demo capabilities.

## Prototype reset

Provide a visible demo scenario selector, persona selector and Reset action. Reset returns to Alex, Development, HTTP 200 and revision rev18 with no unsaved changes. It also resets pending invitations, comments and conflicts. Identify these as demo controls rather than shipping app navigation.
