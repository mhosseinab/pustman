# Deploy deltas — pustman-v1

Env changes the code needs, one row per step with a non-`none` Deploy delta. Appended by the
`integrator` at merge; statuses advanced by `/promote`. Types: **codified** (repo config),
**scripted** (a CLI line a human runs — verbatim), **manual** (runbook line; ≤ 2 per promotion).
`promote-check` refuses a window with any `pending` row.

This is a desktop app: there is no staging and no deploy. **"Staging" means the build host** — the
machine that compiles `pustman` and runs the human manual smoke (Linux + macOS). A `codified`
delta whose effect is automatic and exercised by the full verify is marked `applied(<evidence>)` by
the integrator at merge (plan D11); a `manual` delta is `applied(<tag>)` only when the human has
done it on the build host.

| Step | Type | What | How to apply | Status |
|---|---|---|---|---|
| S0 | manual | install GPUI system packages on every dev/build host: libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev (this is gate G-DEPS) | the host package manager (e.g. `apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libvulkan-dev`); run by the human | applied(evidence: `scripts/pustman-v1-init.sh --require-gpui` → `GPUI_DEPS=ok`, exit=0 on the build host after the human installed the packages) |
| S0 | codified | `.claude/settings.json` hooks (PreToolUse protect-steps-and-tests, SessionStart progress reminder) | active in the checkout; exercised by `scripts/pustman-v1-verify.sh` and `scripts/promote-check.sh` | applied(auto: full verify `VERIFY OK (no workspace yet)` at S0 merge) |

Status values: `pending` → `applied(<evidence>)` (codified: the full-verify run at merge) or
`applied(<tag>)` (manual: done on the build host before that tag).
