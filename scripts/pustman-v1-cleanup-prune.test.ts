import { afterEach, beforeEach, describe, expect, setDefaultTimeout, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Stale test binaries in target/debug/deps filled the disk twice in one session.
setDefaultTimeout(120_000);

const script = join(import.meta.dir, "pustman-v1-cleanup.sh");
let repo: string;
let deps: string;

function git(...args: string[]) {
	const result = Bun.spawnSync(["git", ...args], { cwd: repo, stderr: "pipe" });
	if (result.exitCode !== 0) {
		throw new Error(`git ${args.join(" ")}: ${result.stderr.toString()}`);
	}
}

function cleanup(): string {
	const result = Bun.spawnSync(["bash", script, "--min-free-gb", "0"], {
		cwd: repo,
		stderr: "pipe",
	});
	return result.stdout.toString() + result.stderr.toString();
}

function binary(name: string, minutesAgo: number, mode = 0o755): string {
	const path = join(deps, name);
	writeFileSync(path, "x", { mode });
	const seconds = Date.now() / 1000 - minutesAgo * 60;
	utimesSync(path, seconds, seconds);
	return path;
}

beforeEach(() => {
	repo = mkdtempSync(join(tmpdir(), "pustman-cleanup-prune-"));
	git("init", "-q", "-b", "pustman-v1/main");
	writeFileSync(join(repo, "README"), "init");
	git("add", "README");
	git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init");
	deps = join(repo, "target", "debug", "deps");
	mkdirSync(deps, { recursive: true });
});

afterEach(() => {
	rmSync(repo, { recursive: true, force: true });
});

describe("pustman-v1-cleanup stale test binaries", () => {
	test("deletes older builds of a test binary and keeps the newest", () => {
		const old = binary("api_model-0123456789abcdef", 120);
		const newest = binary("api_model-fedcba9876543210", 1);
		const output = cleanup();
		expect(existsSync(old)).toBe(false);
		expect(existsSync(newest)).toBe(true);
		expect(output).toContain("PRUNED 1 stale test binaries in target/debug/deps");
	});

	test("keeps binaries of one name linked together, such as a crate's lib and bin tests", () => {
		const lib = binary("pustman-0123456789abcdef", 3);
		const bin = binary("pustman-fedcba9876543210", 1);
		cleanup();
		expect(existsSync(lib)).toBe(true);
		expect(existsSync(bin)).toBe(true);
	});

	test("never touches libraries, dep-info or non-executable files", () => {
		binary("api_model-fedcba9876543210", 1);
		const rlib = binary("libapi_model-0123456789abcdef.rlib", 120);
		const depInfo = binary("api_model-0123456789abcdef.d", 120, 0o644);
		const plain = binary("other-0123456789abcdef", 120, 0o644);
		cleanup();
		expect(existsSync(rlib)).toBe(true);
		expect(existsSync(depInfo)).toBe(true);
		expect(existsSync(plain)).toBe(true);
	});
});
