# Capture your own current Postman baseline

The included originals come from official Postman documentation. They are reference material, not a capture of your installed build. The documentation labels its current branch Latest (v12), but some pages reuse v11 and v10 images. Record your own build before comparing any screen.

## Capture setup

1. Create a disposable demo workspace using the fictional API fixtures in this pack. Use fake people, sample records and placeholder credentials. Do not use a production workspace as the screenshot source.
2. Record app version/build, OS, desktop or web client, selected workspace type, role, plan, theme, app zoom, viewport dimensions and capture date. If the web client uses an agent, record which agent is selected.
3. Start with one consistent 1440 × 1000 workspace viewport. Repeat the main workbench and collaboration drawer at 1280 × 800. This is a proposed review setup, not a Postman requirement.
4. Save full-window PNG originals. Put annotations, arrows and cropped details in separate files so the originals stay intact. Never stretch an image to fit a Figma frame.
5. Capture dark and light themes for the core workbench, a dense table and an error state. Verify any theme-dependent contrast in the actual implementation later.
6. Keep the collection, selected request and response fixture consistent across related captures. For a state transition, capture before and after, and note what action triggered it.

## Priority capture list

| ID | Screen or state | What the capture must show |
| --- | --- | --- |
| B01 | Workspace and collection navigation | Workspace identity, active role, collection/folder hierarchy and current request. |
| B02 | New unsaved request | Empty URL, first action, tab state and save destination. |
| B03 | Successful request | Method, URL, environment, response status/time/size, readable JSON and Send placement. |
| B04 | Request headers and body | Repeated query/header keys, disabled rows, generated headers, JSON editor and validation. |
| B05 | Inherited authorization | Which collection/folder provides auth, how to inspect it and how to override it. |
| B06 | 401 response | Returned HTTP status, API error body and the route to inspect auth. |
| B07 | Transport failure | An actual timeout/DNS/connection error, with no invented HTTP response status. |
| B08 | Variable inspection | Effective value, originating scope and an unresolved variable. Keep secrets masked. |
| B09 | Environment change | Current selection, switching action, variable values and their sharing status. |
| B10 | Secret reference | Local versus shared Vault context, variable sharing state and an inaccessible-secret state. |
| B11 | Test script and result | Pre-request versus post-response context; one passing and one failing assertion. |
| B12 | Runner setup | Selected requests/order, environment, iteration data and run action. |
| B13 | Runner result | Pass/fail/error distinctions, failing request detail and rerun behavior. |
| B14 | Request history and Console | How a prior request is found, its execution context, and its diagnostic detail. |
| B15 | Import preview | Detected input type, destination, warnings and outcome for a cURL or collection import. |
| B16 | Workspace sharing | Specific recipients/audience, roles, visibility and final access summary. |
| B17 | Viewer or denied access | The actual controls available to that role and the request-access route. |
| B18 | Inline collaboration | Comment anchor, thread, mention, resolve/reopen and navigation back to the object. |
| B19 | Change review | Source and destination elements, before/after diff, reviewer status and review action. |
| B20 | Conflict resolution | Base/source/destination context if available, chosen resolution and confirmation. Record which version-control system this belongs to. |
| B21 | Local/Cloud state | Project or branch identity, connected folder, current view, pending changes and synchronization status. |
| B22 | Offline or permission change | A recoverable unsent change, access revoked after opening, or failed synchronization. Do not fabricate a current Postman state that you cannot reproduce. |
| B23 | Shared run handoff | A teammate opening a run with the correct collection, environment identity and revision context. |
| B24 | Keyboard navigation | Visible focus in tree, tabs, key/value table, drawer and dialog. Pair a screenshot with short recorded keystrokes. |

## Team collaboration capture rules

- Use two demo accounts or two people who consent to participate. Do not send invitations or notifications to real coworkers just to populate a mockup.
- Capture what an Editor sees and what a Viewer sees. A screenshot of an Admin screen does not prove the same action is available to everyone.
- Distinguish workspace visibility, membership role, collection/environment permission and secret access. They are different design questions.
- Label a Postman collection/environment fork separately from a local Git branch. Likewise, label a Postman element pull request separately from a GitHub or GitLab pull request.
- A comment in a cloud view is not evidence that the same comment exists in local mode. Capture any context switch and its effect.
- Use old/new values such as `page_size: 25 → 50` for conflict examples. No tokens or private endpoints are needed to demonstrate conflict UX.

## Capture record template

```json
{
  "id": "B03",
  "filename": "B03_request_success_desktop_light.png",
  "captured_at": "replace with ISO timestamp",
  "app_version": "record actual build",
  "client": "desktop or web",
  "os": "record OS and version",
  "plan": "record relevant plan",
  "workspace_visibility": "record visible setting",
  "actor_role": "record actual role",
  "mode": "local or cloud, if applicable",
  "theme": "light",
  "viewport_css_px": [1440, 1000],
  "image_px": [1440, 1000],
  "zoom": "100%",
  "fixture": "fictional fixture identifier",
  "trigger": "action immediately before capture",
  "observed_behavior": "what the actual app did",
  "design_hypothesis": "an improvement to test, separate from observation",
  "sensitive_data_checked": true,
  "notes": "limitations, unexpected behavior, missing state"
}
```

## Import references into Figma

Create a `00 References / Current Postman` section. Place each original at its native aspect ratio, with its source page, date retrieved/captured, source version and reference ID alongside it. Keep `Observed behavior`, `Proposed behavior` and `Open question` as separate notes. For each redesigned flow, link the requirement and state IDs; do not ask the agent to infer business rules from pixels alone.

The supplied images are third-party reference material. This pack does not grant a license to reuse Postman's branding or screenshot assets in a new product.
