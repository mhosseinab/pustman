# 03 · Create the foundations

**Inputs:** Design system spec, selected layout direction, operating contract.  
**Scope:** `03 Foundations`. Shared-system owner only.  
**Review gate:** Real variable modes and a readable dense UI.

```text
Follow the operating contract and implement the supplied design_system.md in 03 Foundations. Inspect existing variables first; reuse matching definitions instead of creating duplicates.

Create semantic color variables for surfaces, text, borders, emphasis, focus, selection and status with Light and Dark modes. Follow the naming and alias structure in the spec. Keep HTTP method colors distinct from request-result severity. Never use color alone to communicate method, permission, status or failure.

Create the specified spacing, radius, size and typography foundations. Use a legible UI typeface and a suitable code typeface available in this Figma file. If a requested font is unavailable, use a documented fallback and record it. Do not silently substitute a different typography system.

Make small foundation examples containing a long URL, JSON, a key/value row, a selected navigation item, a member row and an inline validation error. These examples should test practical density, not serve as a decorative brand board.

Show both themes and all stated text/status combinations. Calculate contrast when tools permit it; otherwise label the contrast check as pending with the exact foreground/background pair. Never declare WCAG compliance from appearance alone.

Completion criteria: variables have meaningful names, modes change the examples correctly, spacing is consistent, values follow the spec, and any unavailable feature/font or unverified contrast pair is listed. Do not create screen-specific one-off tokens without recording why a semantic token is insufficient.
```
