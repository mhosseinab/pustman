import { afterEach, beforeEach, describe, expect, setDefaultTimeout, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// A cloud session's VM has ~30 GB of disk in all, so the host's 80 GB gate would block every
// dispatch there; one step worktree needs at most ~10 GB.
setDefaultTimeout(120_000);

const script = join(import.meta.dir, "pustman-v1-cleanup.sh");
let repo: string;

function git(...args: string[]) {
	const result = Bun.spawnSync(["git", ...args], { cwd: repo, stderr: "pipe" });
	if (result.exitCode !== 0) {
		throw new Error(`git ${args.join(" ")}: ${result.stderr.toString()}`);
	}
}

function cleanup(env: Record<string, string>, ...args: string[]): string {
	const { CLAUDE_CODE_REMOTE: _ignored, ...hostEnv } = process.env;
	const result = Bun.spawnSync(["bash", script, ...args], {
		cwd: repo,
		env: { ...hostEnv, ...env },
		stderr: "pipe",
	});
	return result.stdout.toString() + result.stderr.toString();
}

beforeEach(() => {
	repo = mkdtempSync(join(tmpdir(), "pustman-cleanup-cloud-"));
	git("init", "-q", "-b", "pustman-v1/main");
	writeFileSync(join(repo, "README"), "init");
	git("add", "README");
	git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init");
});

afterEach(() => {
	rmSync(repo, { recursive: true, force: true });
});

describe("pustman-v1-cleanup disk gate", () => {
	test("a cloud session gates on 15 GB free", () => {
		expect(cleanup({ CLAUDE_CODE_REMOTE: "true" })).toContain("need 15G");
	});

	test("the build host gates on 80 GB free", () => {
		expect(cleanup({})).toContain("need 80G");
	});

	test("an explicit minimum wins over the cloud default", () => {
		expect(cleanup({ CLAUDE_CODE_REMOTE: "true" }, "--min-free-gb", "3")).toContain("need 3G");
	});
});
