# 11 · Check themes, resizing and keyboard use

**Inputs:** Approved screens, design system, accessibility requirements, state matrix.  
**Scope:** Existing screens and required size/theme variants.  
**Review gate:** The system works beyond its ideal screenshot.

```text
Follow the operating contract. Audit and fix the approved core screens at 1440×900 and 1280×800, plus a 1920×1080 wide view of the workbench. Use the existing responsive rules; document any new rule as a proposal. Do not redesign the information architecture during this pass.

Switch Light and Dark variable modes on real screen instances. Check text, borders, selection, method badges, success/error status, focus rings, code syntax and overlays. List contrast measurements where verified. Treat unknown pairs as pending; do not claim blanket accessibility compliance.

Test long URLs, nested collections, long member/request names, long localized labels, many tabs, many key/value rows, deeply nested JSON, open comments and a visible console. Determine which area scrolls, which truncates with a recovery path and which collapses. Keep request identity, active environment and important actions discoverable.

Document the keyboard sequence for selecting a request, editing the URL, sending/cancelling, inspecting a response, opening/closing a review panel and resolving a modal. Show visible focus, sensible focus order, escape behavior and focus restoration. Ensure meaning is not conveyed by color alone.

Check dense controls against the target sizes and spacing exceptions in the accessibility spec. Maintain readable content; do not solve overflow by shrinking every label.

Completion criteria: named size/theme/state combinations are inspected, issues are fixed within scope, any remaining risks are documented with frame IDs, and the keyboard behavior is specific enough to implement and test.
```
