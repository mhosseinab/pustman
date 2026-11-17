# 08 · Design team workspace and access

**Inputs:** Collaboration spec, effective-capabilities matrix, member fixtures, screen specs.  
**Scope:** Team/workspace overview and members/access on `06 Team collaboration`; permission errors on `07 Edge states`.  
**Review gate:** Roles, resource scope and actual capabilities agree.

```text
Follow the operating contract. Build the team workspace and access-management screens from collaboration_spec.md using Orbit Labs / Commerce API and the supplied member fixtures.

Alex is an Editor, Mina is an Editor/reviewer, Sam is a Viewer and Jo is an Admin in this scenario. Treat those as labels. For each action, use the effective-capabilities matrix and resource scope from the spec; do not infer permissions from the label or claim these are verified current Postman roles.

Show the team/workspace context, resources, member list, membership/access state, invitation flow and role/access-change flow only to the extent defined in the spec. Distinguish workspace membership from access to an individual collection or other resource when the spec does so. Include pending invite, expired/revoked invite, restricted access and permission-change states where listed.

Create side-by-side views of the same resource for Alex, Sam and Jo. Make available actions discoverable and unavailable actions understandable. Do not hide so much context that Sam cannot understand a shared artifact. Use an explicit request-access or contact-owner path only if the spec defines it.

Document the intended result of permission revocation while someone is editing. Preserve local work or offer the recovery defined in the spec; do not assume the UI alone enforces authorization. Annotate required server-side checks for handoff without exposing implementation details in user-facing copy.

Completion criteria: each sensitive action maps to an explicit capability, role/resource scopes are clear, invite/access errors have recovery, and the same resource has coherent experiences for editor, viewer and admin personas.
```
