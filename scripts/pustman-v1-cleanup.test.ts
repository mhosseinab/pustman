import { afterEach, beforeEach, describe, expect, setDefaultTimeout, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Each test runs a dozen real git commands; on a host busy with cargo builds one git call can
// wait seconds on IO, far past bun's 5 s default.
setDefaultTimeout(120_000);

const script = join(import.meta.dir, "pustman-v1-cleanup.sh");
let repo: string;

function git(cwd: string, ...args: string[]): string {
	const result = Bun.spawnSync(["git", ...args], { cwd, stderr: "pipe" });
	if (result.exitCode !== 0) {
		throw new Error(`git ${args.join(" ")}: ${result.stderr.toString()}`);
	}
	return result.stdout.toString().trim();
}

function commit(cwd: string, file: string, message: string) {
	writeFileSync(join(cwd, file), message);
	git(cwd, "add", file);
	git(cwd, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", message);
}

/** A step worktree on branch pustman-v1/<step>; `merged` squash-lands its work on main. */
function addStep(step: string, merged: boolean): string {
	const worktree = join(repo, ".claude/worktrees/pustman-v1", step);
	git(repo, "worktree", "add", "-q", "-b", `pustman-v1/${step}`, worktree, "pustman-v1/main");
	commit(worktree, `${step}.txt`, `${step} work`);
	if (merged) {
		commit(repo, `${step}.txt`, `${step}: the step`);
	}
	return worktree;
}

function cleanup(...args: string[]) {
	const result = Bun.spawnSync(["bash", script, ...args], { cwd: repo, stderr: "pipe" });
	return { code: result.exitCode, out: result.stdout.toString() + result.stderr.toString() };
}

beforeEach(() => {
	repo = mkdtempSync(join(tmpdir(), "pustman-cleanup-"));
	git(repo, "init", "-q", "-b", "pustman-v1/main");
	commit(repo, "README", "init");
});

afterEach(() => {
	rmSync(repo, { recursive: true, force: true });
});

describe("pustman-v1-cleanup", () => {
	test("removes a merged step's clean worktree but keeps its branch", () => {
		const worktree = addStep("S1", true);
		// Workers leave the shared jobs cap untracked in their worktree; it is not work.
		rmSync(join(worktree, ".cargo"), { recursive: true, force: true });
		Bun.spawnSync(["mkdir", "-p", join(worktree, ".cargo")]);
		writeFileSync(join(worktree, ".cargo/config.toml"), "[build]\njobs = 4\n");

		const { code, out } = cleanup("--min-free-gb", "0");

		expect(out).toContain("REMOVED S1");
		expect(code).toBe(0);
		expect(existsSync(worktree)).toBe(false);
		expect(git(repo, "branch", "--list", "pustman-v1/S1")).toContain("pustman-v1/S1");
	});

	test("keeps the worktree of a step that has not merged", () => {
		const worktree = addStep("S2", false);

		const { out } = cleanup("--min-free-gb", "0");

		expect(out).not.toContain("REMOVED S2");
		expect(existsSync(worktree)).toBe(true);
	});

	test("does not mistake S3a's merge for S3's", () => {
		const worktree = addStep("S3", false);
		commit(repo, "other.txt", "S3a: a sibling step");

		cleanup("--min-free-gb", "0");

		expect(existsSync(worktree)).toBe(true);
	});

	test("keeps a merged step's worktree that still has uncommitted changes", () => {
		const worktree = addStep("S4", true);
		writeFileSync(join(worktree, "S4.txt"), "unsaved edit");

		const { out } = cleanup("--min-free-gb", "0");

		expect(out).toContain("KEEP S4 dirty");
		expect(existsSync(worktree)).toBe(true);
	});

	test("fails with LOW-DISK when free space is under the minimum", () => {
		const { code, out } = cleanup("--min-free-gb", "999999999");

		expect(code).toBe(1);
		expect(out).toContain("CLEANUP LOW-DISK");
	});

	test("reports OK when free space is above the minimum", () => {
		const { code, out } = cleanup("--min-free-gb", "0");

		expect(code).toBe(0);
		expect(out).toContain("CLEANUP OK");
	});
});
