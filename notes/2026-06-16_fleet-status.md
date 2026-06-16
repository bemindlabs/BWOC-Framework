# 2026-06-16 — `bwoc fleet status` (issue #297)

`bwoc fleet` and `bwoc supervise status` were reported as returning empty output.
Root cause: there was no fleet **status** view at all. `bwoc fleet` is a
subcommand group whose only member was `health`, so bare `bwoc fleet` just
printed clap help; `bwoc supervise status` parsed `status` as an *agent name*
(supervise takes a positional agent), yielding "no agent named 'status'".

## What changed

- **`bwoc fleet status`** — a per-agent overview answering "which agents are
  stuck?": `AGENT  BACKEND  STATUS  PENDING  LAST-SEEN`, plus `--json`. Both
  forms are non-TTY-friendly (plain text / JSON, no TUI). Pending count + last-seen
  come from each agent's inbox via the shared `AgentEntry::inbox_path` resolver
  (the one added for #302); last-seen is the inbox file's mtime age — the best
  "last activity" signal without a live daemon probe.
- **Bare `bwoc fleet` now defaults to `status`** — `Fleet(Option<FleetCommand>)`;
  `None` runs the status overview instead of printing help.

## Decisions

- **Status, not supervisor-introspection.** "running/idle" would need a live
  daemon/launchd probe (platform-specific, racy). The registry status + pending
  count + inbox-age trio already answers the operator's real question (high
  pending + old last-seen = stuck) with zero new dependencies (Mattaññutā).
- **Reused the #302 resolver** rather than re-deriving the inbox path — one
  source of truth for "where the inbox lives".
- Left `bwoc supervise` alone: it is deliberately per-agent (supervise one
  daemon). The fleet-wide view belongs under `fleet`.

## Status / deferred

- Live running/idle state (daemon up?) is a later slice if a supervisor process
  registry is added.

## Related

- issue #297; `crates/bwoc-cli/src/fleet.rs`, `crates/bwoc-cli/src/main.rs`
