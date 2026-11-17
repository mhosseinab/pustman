# Design system and Figma structure

This is a proposed starting system for the redesign. Colors, typography and sizing are not claimed to be Postman's official design tokens.

## File organization

00 Brief and decisions; 01 Evidence; 02 Flows and IA; 03 Foundations; 04 Components; 05 Workbench; 06 Team collaboration; 07 Edge states; 08 Prototype; 09 Handoff; 99 Archive.

Keep official screenshots only on Evidence. Mark them "Reference - original Postman UI". Keep new screens entirely editable. An image of a UI is not a substitute for components, text, variables or auto layout.

Use frames such as `Workbench / HTTP / Success / Dark / 1440` and `Review / Conflict / Editor / Light / 1280`. Put requirement IDs and decision links in descriptions. Keep main components on Components; use instances on screens.

## Foundation tokens

The JSON in design_tokens.json is a human/agent-readable seed, not a claim that Figma imports this format natively. Have the agent create or map variables explicitly. Bind component fills, text, borders, spacing and radii to tokens rather than duplicating literal values.

- Spacing scale: 4, 8, 12, 16, 20, 24, 32, 40 px.
- Radius: 4 px for dense controls, 6 px for panels/menus, 10 px for dialogs. Avoid large decorative rounding across a technical workbench.
- Density: compact rows 32 px; comfortable rows 40 px. Icon hit areas at least 24 x 24 CSS-pixel equivalent; aim for 32 px in the main desktop toolbar.
- Typography proposal: Inter for UI and JetBrains Mono for URL/code. This is a new-design choice, not a claim about the current app font. If unavailable, use a documented substitute and keep every frame consistent.
- Type scale: 12/16 labels and metadata; 13/20 code; 14/20 primary UI; 16/24 section titles; 24/32 workspace headings. Do not use tiny text to squeeze in another column.
- Motion: 120-180 ms for small transitions; respect reduced-motion settings in runnable prototypes. Avoid animation during response reading.

Theme roles: canvas, surface, raised, text/primary, text/secondary, border/subtle, border/control, accent/action, accent/on-action, success, warning, error, info, focus and selection. Method labels always include text: GET, POST, PUT, PATCH, DELETE. HTTP method color alone carries no meaning.

## Component inventory

| Family | Components | Key properties/states |
| --- | --- | --- |
| Shell | WorkspaceSwitcher, GlobalSearch, NavItem, Breadcrumb, StatusStrip | Active, focused, read-only, offline; local/cloud context |
| Navigation | CollectionTreeRow, RequestTab, OverflowMenu, CommandResult | Depth, method, selected, expanded, dirty, missing access |
| Request | MethodSelect, UrlInput, SendButton, RequestConfigTab, KeyValueRow | Enabled, disabled, invalid, sending/cancel, secret, inherited |
| Context | EnvironmentPicker, VariableToken, VariableInspector, AuthSource | Resolved, missing, shadowed, local/shared, inherited/overridden |
| Response | ResponseStatus, TimingSummary, CodeViewer, ResponseTab, TestRow | HTTP response versus transport failure, pass/fail, loading, huge body |
| Collections | CollectionCard/Row, ExampleRow, RunConfigRow, RunSummary | Empty, active, selected, failure, skipped, cancelled |
| Collaboration | MemberRow, RoleBadge, InviteRow, AccessSource, AudiencePreview | Pending, effective permission, expired, restricted, access lost |
| Review | CommentThread, MentionItem, RevisionChip, ReviewSummary, DiffRow, ConflictChoice | Resolved, stale, pending, approved, conflict, merge failed |
| Feedback | Banner, Toast, EmptyState, InlineError, Dialog, Drawer, Skeleton | Focus management, live status, action, dismissibility |

Avoid a variant for every combination. Use variant properties for meaningful behavior; text, boolean and instance-swap properties for labels, optional icons and content. Use nested components. Code content can be text/editor primitives; do not force every line of JSON into a component instance.

## Workbench geometry

Start at 1440 x 900. Proposed regions: 48 px app bar; 48 px navigation rail; 248 px collection sidebar; 40 px request-tab row; flexible main workbench; 28 px status bar. Within the main region, use a 72 px context/URL band, 36 px configuration tabs, a resizable request editor and a response section. These are hypotheses to test, not immovable constraints.

Keep at least 640 px for useful editor content when a contextual collaboration panel is open. At 1440 px, a 320 px panel may require collapsing the collection tree. At 1280 x 800, collapse that tree before squeezing code and controls. Below the specified minimum, use an intentional focused view and navigation drawer. Mobile parity is outside the first milestone.

At 200% zoom, preserve controls, focus and access to all content; horizontal scrolling may be appropriate inside code and data tables, not as a replacement for basic app navigation. Record actual implementation constraints during prototype testing.

## Collaboration layouts

Comments use a 320-360 px contextual panel, with clear artifact/revision context. Members, review and conflict resolution use the main content area. A wide diff can switch from side-by-side to unified; never shrink text simply to keep two columns.

The team workspace overview emphasizes first useful request, environment setup and recent relevant changes. Avoid a dashboard of vanity metrics. The inbox is a task queue, not a social feed.

## Accessibility contract

Use WCAG 2.2 AA as the web-prototype target. For a desktop implementation, apply the appropriate native accessibility semantics and review WCAG2ICT guidance. A Figma screenshot cannot certify accessibility.

Require 4.5:1 text contrast for normal text, 3:1 for qualifying large text, and appropriate 3:1 contrast for meaningful non-text controls. Keep visible keyboard focus and prevent it being hidden by sticky regions. Meet the 24 x 24 minimum pointer-target criterion or a documented exception; prefer larger comfortable targets where space allows.

Annotate keyboard order, tab/arrow-key behavior for tabs and trees, accessible control names, dialog focus trap/return, live announcements of asynchronous outcomes, error associations and non-color state cues. Test these in the runnable prototype; they are not proven by layer names.

Source: https://www.w3.org/TR/WCAG22/ and https://www.w3.org/TR/wcag2ict-22/ .

## Structural acceptance

- Main controls and repeated rows are component instances.
- Layout containers use auto layout and intentional constraints; resizing preserves hierarchy.
- Color roles use variables with light/dark modes; typography uses text styles.
- Every interactive component has focus and disabled behavior; relevant controls also have loading, error and selected variants.
- All actual text remains editable; no complete-UI screenshot is inside a final screen frame.
- The canonical workbench passes visual inspection at both target sizes before replication.
- Foundation changes have one owner; parallel agents may work on separate screens after components stabilize.
