---
name: explain
description: Walk me through a module, a step's diff, or a data flow from the code itself, with file:line anchors — comprehension on demand for a codebase the loop wrote. Use for "explain S12", "explain src/orders/", "how does a refund flow?", "what did last night's steps change?".
argument-hint: <path | S<N> | question>
context: fork
agent: code-explainer
---

Explain `$ARGUMENTS` for a maintainer who did not write this code.

Resolve the target: a path or module → read it and its callers two levels out; `S<N>` → find the
squash commit `S<N>:` on `pustman-v1/main` (`git log --oneline --grep '^S<N>:'`) and explain the diff
plus its step record (`docs/progress/pustman-v1/steps/S<N>.md`); a question → find the entry point and follow the code.

Build the explanation from code. Use `docs/ARCHITECTURE.md`, `docs/decisions/`, the step records and the progress
log only as pointers; where they disagree with the code, say so explicitly.

Output ≤ 1 screen: what it does · where (5–10 `file:line` anchors in reading order) · invariants &
risks · how to debug it at 02:00 · docs that disagree · for a step: what changed, why, what a
reviewer should have worried about, whether the row's evidence matches what you see.
