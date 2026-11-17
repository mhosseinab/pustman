# ADR-0001: Workspace files are pretty-printed JSON (D1)

- **Status:** accepted
- **Date:** 2026-11-17
- **Source:** plan §4.D1 (PRD §6)

## Context

The workspace on-disk format had to be fixed before `workspace_store` and every import/export path
is built. The fork was JSON or YAML. The format must be Git-friendly (one file per item, reviewable
diffs), represent every request kind (gRPC/WebSocket/MQTT items that Postman v2.1 cannot represent),
and fit the parser the repo already uses (`serde_json`). Postman v2.1 remains the import/export
format, so the on-disk format is free to be something else (PRD §6 note).

## Decision

**JSON.** Pretty-printed with 2-space indent, stable struct field order, maps as `BTreeMap` (or
sorted `IndexMap`), a trailing `\n`, and atomic writes: temp file in the same directory, fsync, then
rename. The layout is the PRD §6.1 folder of files (`pustman.json`, `collections/**`,
`environments/**`, `protos/**`) — one file per request, folder, collection and environment, with the
extension encoding the item type. Postman v2.1 is the **import/export** format only.

## Consequences

- Saving an unchanged item produces a zero-byte diff (REQ-WSP-02); Git reviews show only real changes.
- One parser across the product (`serde_json::from_slice`, never `from_reader` — the tripwire counts
  the latter).
- Later milestones add item files additively (`.websocket.json`, `.grpc.json`, …); `format_version`
  gates breaking changes (REQ-WSP-04).
- A later step must honor: deterministic serialization is a test assertion, not a hope.

## Dissent / alternatives rejected

- YAML — rejected: weaker deterministic-output guarantees, a second parser dependency, and Postman
  interop is JSON anyway.
- One file per collection (v2.1-shaped) — rejected: conflicts on every concurrent edit and cannot
  represent the later protocols (PRD §6 note).
