@AGENTS.md

## Claude Code specifics

- Canonical skills, rules and agent roles live in `.agents/` (see AGENTS.md). `.claude/skills/*` and
  `.claude/rules/*` are symlinks into it; `.claude/agents/*.md` are wrappers. Edit `.agents/`, not the links.
- For any UI work, load the `rust-gpui-ui-ux` skill before writing code, and follow its "Design source"
  section for `docs/postman_redesign_pack/`.
- Before claiming a UI change works, run the relevant GPUI test and say which checklist items
  (`.agents/skills/rust-gpui-ui-ux/references/ux-checklist.md`) you verified and which you could not.
