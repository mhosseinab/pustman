import { describe, expect, test } from "bun:test";
import { globToRegExp, parseRulePaths, ruleKeyForPath, rulesMatching } from "./rules.ts";

describe("globToRegExp", () => {
	test("a leading **/ also matches a file at the repo root", () => {
		const glob = globToRegExp("**/Cargo.toml");
		expect(glob.test("Cargo.toml")).toBe(true);
		expect(glob.test("crates/theme/Cargo.toml")).toBe(true);
		expect(glob.test("crates/theme/Cargo.toml.bak")).toBe(false);
	});

	test("a single * never crosses a directory boundary", () => {
		const glob = globToRegExp("assets/keymaps/*.json");
		expect(glob.test("assets/keymaps/default-linux.json")).toBe(true);
		expect(glob.test("assets/keymaps/old/default-linux.json")).toBe(false);
	});

	test("a middle /**/ matches zero or more directories", () => {
		const glob = globToRegExp("crates/*_panel/**/*.rs");
		expect(glob.test("crates/history_panel/src/history_panel.rs")).toBe(true);
		expect(glob.test("crates/history_panel/lib.rs")).toBe(true);
		expect(glob.test("crates/request_editor/src/editor.rs")).toBe(false);
	});

	test("dots are literal", () => {
		expect(globToRegExp("**/*_tests.rs").test("crates/x/src/a_testsxrs")).toBe(false);
	});
});

describe("parseRulePaths", () => {
	test("reads the quoted paths list from frontmatter", () => {
		const markdown = '---\npaths:\n  - "**/tests/**/*.rs"\n  - "**/*_tests.rs"\n---\n\n# Test traps\n- "not/a/path"\n';
		expect(parseRulePaths(markdown)).toEqual(["**/tests/**/*.rs", "**/*_tests.rs"]);
	});

	test("a rule without frontmatter has no paths", () => {
		expect(parseRulePaths("# Just prose\n")).toEqual([]);
	});
});

describe("rulesMatching", () => {
	const rules = [
		{ name: "rust-code.md", globs: ["**/*.rs"] },
		{ name: "tests.md", globs: ["**/tests/**/*.rs", "**/*_tests.rs"] },
		{ name: "keymaps.md", globs: ["assets/keymaps/*.json"] },
	];

	test("a test file matches both the rust and the tests rule", () => {
		expect(rulesMatching("crates/theme/src/theme_tests.rs", rules)).toEqual(["rust-code.md", "tests.md"]);
	});

	test("an unrelated file matches no rule", () => {
		expect(rulesMatching("docs/ARCHITECTURE.md", rules)).toEqual([]);
	});
});

describe("ruleKeyForPath", () => {
	test("the canonical rule file and its .claude symlink path give the same key", () => {
		expect(ruleKeyForPath("/repo/.agents/rules/tests.md")).toBe("tests.md");
		expect(ruleKeyForPath("/repo/.claude/worktrees/pustman-v1/S12/.claude/rules/tests.md")).toBe("tests.md");
	});

	test("a file that is not a rule has no key", () => {
		expect(ruleKeyForPath("/repo/docs/tests.md")).toBeNull();
	});
});
