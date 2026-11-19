@AGENTS.md

## Claude Code specifics

- Canonical skills, rules and agent roles live in `.agents/` (see AGENTS.md). `.claude/skills/*` and
  `.claude/rules/*` are symlinks into it; `.claude/agents/*.md` are wrappers. Edit `.agents/`, not the links.
- After editing `.claude/agents/*.md`, run `bun scripts/pi-sync-agents.ts` so `.pi/agents/` follows.
