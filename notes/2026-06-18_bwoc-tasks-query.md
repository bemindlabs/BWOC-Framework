# 2026-06-18 — `bwoc tasks` fleet-wide task-status query (issue #300)

The per-team `bwoc task list <team>` answers "what's on *this* team's list", but
there was no way to ask "what's open / in-progress / done **across the fleet**".
The reporter searched every agent's `.bwoc/` for a `status:` field and found
nothing — because task state lives in the **team** store, not per-agent.

## What changed

- **`bwoc tasks`** (`crates/bwoc-cli/src/tasks.rs`) — a read-only, cross-team
  aggregate. Scans every `.bwoc/teams/<id>/tasks.jsonl`, loads each via the
  existing `bwoc_core::team::parse_tasks`, and prints
  `TEAM  ID  STATE  CLAIMED-BY  TITLE` (or `--json`). Filters:
  `--agent <id>` (claimant; bare name auto-prefixed) and `--state`
  (`pending` | `in_progress` | `completed`, with `in-progress`/`done` aliases).
  Distinct command from the singular per-team `bwoc task`.

## Decisions

- **Query over the existing store, not a new registry.** The issue proposed "add
  a task registry with explicit states" — but the team `tasks.jsonl` *already*
  carries explicit `state` + `claimed_by` (Pending → InProgress → Completed). The
  gap was a *fleet-wide reader*, so this adds exactly that and nothing more
  (Mattaññutā). No schema change.
- **Reused `team::parse_tasks`** — one parser for the JSONL, shared with
  `bwoc task` and the a2a task bridge.

## Status / deferred

- The model has no `failed`/`assigned` state (the issue's proposal listed them).
  Pending → in_progress → completed is the current lifecycle; adding states is a
  `bwoc_core::team` model change, separate from this query command.
- Per-agent inbox tasks (`interconnect/inbox/*.task`) are a different channel
  (the worker queue) — not part of the team task store this queries.

## Related

- issue #300; `crates/bwoc-cli/src/tasks.rs`, `crates/bwoc-cli/src/main.rs`,
  `crates/bwoc-core/src/team.rs`
