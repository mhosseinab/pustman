// Pure helpers for the path-scoped rules guard. Kept free of pi imports so `bun test` runs them.

export interface Rule {
	name: string;
	globs: string[];
}

// Hand-rolled because pi may run under bun, where `path.matchesGlob` is not guaranteed. Supports
// exactly what `.agents/rules/*.md` uses: `**/`, `/**/`, `*`, `?`.
export function globToRegExp(glob: string): RegExp {
	let pattern = "";
	for (let index = 0; index < glob.length; index++) {
		const char = glob[index];
		if (char === "*" && glob[index + 1] === "*") {
			const followedBySlash = glob[index + 2] === "/";
			pattern += followedBySlash ? "(?:.*/)?" : ".*";
			index += followedBySlash ? 2 : 1;
		} else if (char === "*") {
			pattern += "[^/]*";
		} else if (char === "?") {
			pattern += "[^/]";
		} else {
			pattern += char.replace(/[.+^${}()|[\]\\]/g, "\\$&");
		}
	}
	return new RegExp(`^${pattern}$`);
}

export function parseRulePaths(markdown: string): string[] {
	const frontmatter = /^---\n([\s\S]*?)\n---/.exec(markdown);
	if (!frontmatter) return [];
	const paths: string[] = [];
	let inPaths = false;
	for (const line of frontmatter[1].split("\n")) {
		if (/^paths:\s*$/.test(line)) {
			inPaths = true;
			continue;
		}
		const item = /^\s+-\s+["']?([^"']+)["']?\s*$/.exec(line);
		if (inPaths && item) paths.push(item[1]);
		else if (!/^\s/.test(line)) inPaths = false;
	}
	return paths;
}

export function rulesMatching(relativePath: string, rules: Rule[]): string[] {
	return rules
		.filter((rule) => rule.globs.some((glob) => globToRegExp(glob).test(relativePath)))
		.map((rule) => rule.name);
}

// The same rule is reachable as `.agents/rules/x.md`, as the `.claude/rules/x.md` symlink, and from
// the main checkout or any worktree; reading any of those counts as having read rule `x.md`.
export function ruleKeyForPath(filePath: string): string | null {
	const match = /\/\.(?:agents|claude)\/rules\/([^/]+\.md)$/.exec(filePath);
	return match ? match[1] : null;
}
