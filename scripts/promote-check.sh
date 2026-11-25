#!/usr/bin/env bash
# promote-check — the mechanical promotion gate. PASS/FAIL per item; exit 1 on any FAIL.
# Usage: scripts/promote-check.sh <candidate-sha> [--doc-truth PASS|FAIL]
#
# Pustman v1 is a desktop app: there is no server, no staging environment and no deploy step.
# "Staging" for this app means the BUILD HOST — the machine that compiles `pustman` and runs the
# human manual smoke (plan §10, D11). A delta is "applied" only when the build host (or every dev
# host it names) has it. A promotion is: this gate → tag `release/pustman-v1-<n>` →
# `cargo build --release -p pustman` → manual smoke on Linux + macOS.
set -u
SHA="${1:?candidate sha required}"; shift || true
DOC_TRUTH="UNKNOWN"; [ "${1:-}" = "--doc-truth" ] && DOC_TRUTH="${2:-UNKNOWN}"
SLUG="pustman-v1"
FULL_VERIFY="scripts/pustman-v1-verify.sh"                        # merge gate (fmt, clippy, tests, tripwire)
ENV_VALIDATE="scripts/pustman-v1-init.sh --require-gpui"          # build host has the GPUI system libs (gate G-DEPS)
TRIPWIRE_CMD="scripts/pustman-v1-tripwire.sh | awk '{print \$2}'" # prints the tripwire count
TRIPWIRE_BASELINE=0
TEST_PATHS="crates/*/tests crates/*/src/*_tests.rs crates/*/src/**/*_tests.rs crates/*/src/test_support.rs"
MIGRATIONS_DIR="crates/storage/migrations"
LEDGER="DEPLOY_DELTAS.md"
MAX_MANUAL=2
fail=0; n=0
item(){ n=$((n+1)); if [ "$1" = PASS ]; then printf '%d. PASS — %s\n' "$n" "$2"; else printf '%d. FAIL — %s\n' "$n" "$2"; fail=1; fi; }

# 0. placeholders left?
if grep -v '^[[:space:]]*#' "$0" | grep -q '"<[a-z][a-z ]*>"'; then echo "0. FAIL — placeholders left in $(basename "$0"); an empty gate is worse than no gate"; exit 1; fi

LAST_TAG="$(git tag -l "release/${SLUG}-*" --sort=-v:refname | head -1)"
RANGE="${LAST_TAG:+${LAST_TAG}..}${SHA}"
git merge-base --is-ancestor "$SHA" "${SLUG}/main" 2>/dev/null || { echo "candidate $SHA is not on ${SLUG}/main"; exit 1; }

# 1. full verify at SHA (in a temporary worktree so the driver checkout stays put)
WT="$(mktemp -d)"; git worktree add -q "$WT" "$SHA" && ( cd "$WT" && eval "$FULL_VERIFY" >/tmp/pc_full.log 2>&1 ) && item PASS "full verify green at $SHA" || item FAIL "full verify red — $(tail -1 /tmp/pc_full.log 2>/dev/null)"
git worktree remove -f "$WT" 2>/dev/null || true

# 2. env validator on the build host (probes, not presence)
eval "$ENV_VALIDATE" >/tmp/pc_env.log 2>&1 && item PASS "env validator green on the build host" || item FAIL "env validator — $(grep -m1 -iE 'error|missing|fail' /tmp/pc_env.log)"

# 3. tripwire ≤ baseline
TW="$(eval "$TRIPWIRE_CMD" 2>/dev/null | tr -d '[:space:]')"
[ -n "$TW" ] && [ "$TW" -le "$TRIPWIRE_BASELINE" ] && item PASS "tripwire $TW ≤ baseline $TRIPWIRE_BASELINE" || item FAIL "tripwire $TW > baseline $TRIPWIRE_BASELINE"

# 4. no existing test modified/deleted in window (added is fine) unless the human authorized
#    it. Checked commit by commit: the net diff misses a test file that was added and then
#    edited inside the window. An authorization is a row in the ledger at the candidate.
AUTH_LEDGER="docs/progress/${SLUG}/test-edit-authorizations.tsv"
MOD="$(for c in $(git rev-list --no-merges "$RANGE" 2>/dev/null); do
  git diff-tree --no-commit-id -r --name-status "$c" -- $TEST_PATHS 2>/dev/null | grep -Ev '^A' | awk '{print $NF}'
done | sort -u)"
AUTHORIZED="$(git show "$SHA:$AUTH_LEDGER" 2>/dev/null | awk -F'\t' '!/^#/ && $1 != "path" && NF >= 2 {print $1}' | sort -u)"
UNAUTHORIZED="$(comm -23 <(printf '%s\n' "$MOD" | sed '/^$/d') <(printf '%s\n' "$AUTHORIZED" | sed '/^$/d'))"
WAIVED="$(comm -12 <(printf '%s\n' "$MOD" | sed '/^$/d') <(printf '%s\n' "$AUTHORIZED" | sed '/^$/d') | tr '\n' ' ')"
if [ -n "$UNAUTHORIZED" ]; then item FAIL "modified/deleted tests without a human authorization in $AUTH_LEDGER: $(echo "$UNAUTHORIZED" | tr '\n' ' ')"
elif [ -n "$WAIVED" ]; then item PASS "existing tests modified in window, all human-authorized ($AUTH_LEDGER): $WAIVED"
else item PASS "no existing test modified in window"; fi

# 5. every migration in window has a down
if [ -n "$MIGRATIONS_DIR" ]; then
  MISSING=""; for f in $(git diff --name-only "$RANGE" -- "$MIGRATIONS_DIR" 2>/dev/null); do
    grep -qiE 'down|revert|rollback' "$f" 2>/dev/null || MISSING="$MISSING $f"; done
  [ -z "$MISSING" ] && item PASS "all migrations in window define down" || item FAIL "no down in:$MISSING"
else item PASS "no migrations dir configured"; fi

# 6. every delta in window applied on the build host
PENDING="$(awk '/^\| S[0-9]+/ && /\| *pending *\|/ {print $2}' "$LEDGER" 2>/dev/null | tr '\n' ' ')"
[ -z "$PENDING" ] && item PASS "all deltas applied on the build host" || item FAIL "deltas still pending:$PENDING"

# 7. every deployed tag is an ancestor of the candidate
BAD=""; for t in $(git tag -l "release/${SLUG}-*"); do git merge-base --is-ancestor "$t" "$SHA" || BAD="$BAD $t"; done
[ -z "$BAD" ] && item PASS "all deployed tags are ancestors" || item FAIL "not ancestors (hotfix fork?):$BAD"

# 8. doc-truth result handed in
[ "$DOC_TRUTH" = PASS ] && item PASS "doc-truth PASS" || item FAIL "doc-truth $DOC_TRUTH (run doc-truth-checker and pass --doc-truth PASS)"

# 9. manual deltas in window ≤ MAX_MANUAL
MANUAL="$(grep -cE '^\| S[0-9]+.*\| *manual' "$LEDGER" 2>/dev/null || echo 0)"
[ "$MANUAL" -le "$MAX_MANUAL" ] && item PASS "manual deltas $MANUAL ≤ $MAX_MANUAL" || item FAIL "manual deltas $MANUAL > $MAX_MANUAL"

[ $fail -eq 0 ] && echo "PROMOTE-CHECK $SHA → PROMOTABLE" || { echo "PROMOTE-CHECK $SHA → NOT PROMOTABLE"; exit 1; }
