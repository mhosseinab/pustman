@AGENTS.md

## Claude Code specifics

- Skills live in `.claude/skills/`, path-scoped rules in `.claude/rules/`, subagents in `.claude/agents/`.
- For any UI work, load the `rust-gpui-ui-ux` skill before writing code.
- Before claiming a UI change works, run the relevant GPUI test and say which checklist items
  (`.claude/skills/rust-gpui-ui-ux/references/ux-checklist.md`) you verified and which you could not.
