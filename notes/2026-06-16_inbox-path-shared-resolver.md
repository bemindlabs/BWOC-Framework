# 2026-06-16 — Inbox path: one shared resolver (issue #302)

`bwoc inbox <agent>` was reported reading a different store than the gateway
receiver writes (4 stale vs 89 live messages for agent-anna). Diagnosis on the
live Mac node: the current code (v2.31.0) actually resolves correctly — both the
CLI reader and the a2a writer compute `<workspace>/<entry.path>/.bwoc/inbox.jsonl`
— and `bwoc inbox agent-anna --count` returns the live 88/89. The reported
symptom came from the **stale installed binary (v2.29.0)** and/or a
`gateway-recv` launchd configured with a **hardcoded `--inbox` absolute path**
that can drift from the registry.

## What changed

- **One canonical resolver** — `bwoc_core::workspace::AgentEntry::inbox_path(workspace)`
  (+ `dir()`). The `bwoc inbox` reader (`inbox.rs`, 4 call sites) and the a2a /
  gateway writer (`bwoc-a2a/main.rs::resolve_agent`) now both resolve through it,
  so a reader and writer can't disagree about the path by construction.
- **`bwoc inbox <agent> --path`** — prints the resolved inbox path and exits
  without reading it. External writers derive the exact path the CLI reads —
  `--inbox "$(bwoc inbox agent-anna --path)"` — instead of hardcoding a drifting
  one. This closes the drift class at the deployment boundary.

## Decisions

- **Resolver lives in bwoc-core**, not duplicated in each binary — the writer
  (`bwoc-a2a`) and reader (`bwoc-cli`) share one definition. Yoniso manasikāra:
  the bug was a *consistency* risk between two independently-computed paths;
  the fix is to make them one computation.
- **No path-migration / store-merge code** — there is only ever one inbox file
  per agent; the issue was binary staleness + a hardcoded deployment path, not a
  second store. Don't invent a migration for a problem that isn't there.

## Status / deferred

- Surfaced while testing: one envelope in agent-anna's live inbox is malformed
  JSON (`unexpected end of hex escape`). That's a writer-side data-integrity
  bug (gateway/send wrote a bad `\u` escape), tracked separately — the reader
  already warn-and-skips it.
- The launchd `gateway-recv` units on the Mac should be updated to use
  `--inbox "$(bwoc inbox <agent> --path)"` (deployment change, not framework).

## Related

- issue #302; `crates/bwoc-core/src/workspace.rs`, `crates/bwoc-cli/src/inbox.rs`,
  `crates/bwoc-a2a/src/main.rs`
