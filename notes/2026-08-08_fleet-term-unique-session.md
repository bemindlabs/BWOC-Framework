# 2026-08-08 — `bwoc fleet term`: per-workspace default session name

`bwoc fleet term` defaulted its tmux session to a fixed `bwoc-fleet`, so opening
two fleets at once (from different workspaces) collided — the second refused with
"session already exists". The default is now derived from the workspace, unique
per fleet.

## What changed

- `--session` is now optional. When omitted, the session name defaults to
  **`bwoc-fleet-<slug>-<hash>`** — `slug` = the workspace directory basename
  (sanitized, tmux-safe), `hash` = a 6-hex FNV-1a digest of the canonical
  workspace path. Different workspaces → different sessions (no collision when
  run concurrently); the same workspace → the *same* name (deterministic, so a
  re-run attaches rather than spawning a duplicate).
- `crates/bwoc-cli/src/fleet_term.rs`: `FleetTermArgs.session: Option<String>`,
  `default_session_name` + `sanitize_segment` + `fnv1a_24` (pure, tested).
- `crates/bwoc-cli/src/main.rs`: the `--session` clap arg dropped its static
  `bwoc-fleet` default and became `Option<String>`.

## Decisions

- **Deterministic, not random.** A random suffix would spawn a fresh session on
  every run in the same workspace; deriving from the path keeps one session per
  fleet while still separating distinct fleets.
- **basename + path hash**, not just basename — two different checkouts named
  `bwoc` would otherwise still collide. The hash disambiguates by full path.
- **Explicit `--session` still wins**, unchanged, for anyone scripting a name.

## Tests

`sanitize_segment_is_tmux_safe`, `fnv1a_24_is_deterministic_and_six_hex`,
`default_session_name_is_unique_per_workspace`. fmt + clippy (ws + redteam) clean.

## Related

- [[2026-08-06_fleet-term]] (the original command)
