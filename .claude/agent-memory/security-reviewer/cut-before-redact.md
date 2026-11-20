---
name: cut-before-redact
description: Recurring leak pattern in pustman - truncating text (body caps, prefixes) before running redact_text leaves a partial secret at the cut
metadata:
  type: project
---

When a step caps or truncates text (history body prefix, console previews, exports) and then calls `api_model::redact_text`, a secret straddling the cut survives as a partial prefix (S16a: 1 MB cap cut first, probe stored the first 10 bytes of the canary).

**Why:** redact_text only matches whole secret values; a fragment is invisible to it. Canary tests that scan for the whole value miss it.

**How to apply:** in any review touching caps/truncation + redaction, check the order (redact a window of cap + longest secret, then cut) and probe with a body of `x * (cap-10) + secret`. Also note: secrets_used holds raw values only, so base64 (Basic auth echoes), percent-encoded or JSON-escaped echoes in response bodies are not masked — a shared-redact limitation to flag.

Round-2 fix (window + `straddle_free_prefix`) still had two holes worth probing in any similar fix: (1) `String::from_utf8_lossy` on a byte window that splits a multi-byte char inside a secret turns the tail into U+FFFD, so the straddle check misses it; earlier masks shrink the text and pull the fragment under the cap. (2) Trimming applied even when the window holds the WHOLE body truncates complete small bodies (`"hello"` → `"hell"` if a secret starts with `o`), and the cascade can empty a repetitive body. Probes via a throwaway `#[path]` test module calling the private fns worked; reverting tracked files with `git show >` was blocked by the auto-mode classifier.
