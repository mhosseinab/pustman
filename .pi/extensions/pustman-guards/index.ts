// Pustman's pi adapter for what Claude Code gets from `.claude/settings.json` and `.claude/rules/`:
// - protect-steps-and-tests: runs `.claude/hooks/protect-steps-and-tests.sh` (the single source of
//   the rule) before every edit/write and blocks on exit 2.
// - path-scoped rules: lists `.agents/rules/*.md` in the system prompt and blocks an edit/write to
//   a path a rule's `paths:` globs match until that rule file has been read in this session.
// - compaction reminder: after a compaction, steers the agent back to the progress files.
// Like the Claude hook, this guards the edit/write tools only; `bash` can still write files.

import { spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { parseRulePaths, type Rule, ruleKeyForPath, rulesMatching } from "./rules.ts";

const COMPACTION_REMINDER =
	"Read docs/pustman-v1-progress.md and tail -n 30 docs/progress/pustman-v1/log.md before acting.";

function gitToplevel(filePath: string): string | null {
	let directory = path.dirname(filePath);
	while (!fs.existsSync(directory) && directory !== path.dirname(directory)) {
		directory = path.dirname(directory);
	}
	const result = spawnSync("git", ["-C", directory, "rev-parse", "--show-toplevel"], { encoding: "utf-8" });
	return result.status === 0 ? result.stdout.trim() : null;
}

function loadRules(toplevel: string): Rule[] {
	const rulesDirectory = path.join(toplevel, ".agents", "rules");
	if (!fs.existsSync(rulesDirectory)) return [];
	return fs
		.readdirSync(rulesDirectory)
		.filter((name) => name.endsWith(".md"))
		.sort()
		.map((name) => ({ name, globs: parseRulePaths(fs.readFileSync(path.join(rulesDirectory, name), "utf-8")) }));
}

function runProtectHook(toplevel: string, toolName: string, absolutePath: string): string | null {
	const hook = path.join(toplevel, ".claude", "hooks", "protect-steps-and-tests.sh");
	if (!fs.existsSync(hook)) return null;
	const payload = JSON.stringify({ tool_name: toolName, tool_input: { file_path: absolutePath } });
	const result = spawnSync(hook, [], { cwd: toplevel, input: payload, encoding: "utf-8" });
	if (result.status === 2) return result.stderr.trim() || "blocked by protect-steps-and-tests";
	if (result.status !== 0) {
		// Fail closed: a guard that silently stops guarding is worse than a visible false block.
		return `protect-steps-and-tests hook failed (exit ${result.status}): ${result.stderr.trim() || result.error?.message}`;
	}
	return null;
}

export default function (pi: ExtensionAPI) {
	const rulesRead = new Set<string>();

	pi.on("before_agent_start", async (event, ctx) => {
		const toplevel = gitToplevel(path.join(ctx.cwd, "."));
		if (!toplevel) return undefined;
		const rules = loadRules(toplevel).filter((rule) => rule.globs.length > 0);
		if (rules.length === 0) return undefined;
		const listing = rules.map((rule) => `- .agents/rules/${rule.name} — applies to: ${rule.globs.join(", ")}`).join("\n");
		return {
			systemPrompt: `${event.systemPrompt}\n\n## Path-scoped rules\n\nBefore editing or writing a file, read every rule below whose globs match its repo-relative path. The edit/write tools are blocked until you have.\n${listing}\n`,
		};
	});

	pi.on("tool_call", async (event, ctx) => {
		const rawPath = (event.input as { path?: unknown }).path;
		if (typeof rawPath !== "string") return undefined;
		const absolutePath = path.resolve(ctx.cwd, rawPath.replace(/^@/, ""));

		if (event.toolName === "read") {
			const key = ruleKeyForPath(absolutePath);
			if (key) rulesRead.add(key);
			return undefined;
		}
		if (event.toolName !== "edit" && event.toolName !== "write") return undefined;

		const toplevel = gitToplevel(absolutePath);
		if (!toplevel) return undefined;

		const protectReason = runProtectHook(toplevel, event.toolName === "edit" ? "Edit" : "Write", absolutePath);
		if (protectReason) return { block: true, reason: protectReason };

		const relativePath = path.relative(toplevel, absolutePath);
		const unread = rulesMatching(relativePath, loadRules(toplevel)).filter((name) => !rulesRead.has(name));
		if (unread.length > 0) {
			return {
				block: true,
				reason: `Read ${unread.map((name) => `.agents/rules/${name}`).join(", ")} first (path-scoped rules for ${relativePath}), then retry.`,
			};
		}
		return undefined;
	});

	pi.on("session_compact", async () => {
		pi.sendMessage(
			{ customType: "pustman-compaction-reminder", content: COMPACTION_REMINDER, display: true },
			{ deliverAs: "steer" },
		);
	});
}
