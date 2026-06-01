# 2026-06-01 — Harness `--unrestricted` file access + chat `ask` default

Let the `--chat` agent (and `bwoc-chat` desktop) reach and edit **real files anywhere
on the machine**, with the safety boundary moved from the path sandbox to a per-action
`ask` permission prompt.

## What changed

- `ToolContext` gains a `confine: bool` field. `ToolContext::new` keeps the
  path-traversal sandbox (default); `ToolContext::unconfined` skips the
  `starts_with(workdir)` escape check. Relative paths still resolve against
  `workdir` in both modes — only absolute/escaping paths differ.
- `bwoc-harness --unrestricted` (new flag) builds an unconfined context, in both
  `run()` (batch) and `run_chat_mode()`. Default off → no behaviour change for
  existing batch/Saṅgha runs.
- `--chat` now falls back to an **`ask`** policy (read-only tools `allow`,
  everything else `ask`) when the workdir has no `.bwoc/harness-policy.toml`,
  rather than the batch path's fail-safe deny. Chat always has a frontend to
  answer the Allow/Deny prompt, so this is safe and makes editing usable with no
  setup.
- `bwoc-chat` spawns the harness with `--unrestricted`.

## Decisions

- **Opt-in flag, not default.** Removing confinement globally would weaken the
  sandbox for headless batch/lead runs that have no human to gate writes. The
  flag keeps the safe default and lets the interactive, `ask`-gated chat opt in.
- **Relative paths stay workdir-rooted even when unconfined** — preserves the
  ergonomics of an agent "working in" a directory while still allowing explicit
  absolute paths elsewhere.
- **`ask` default for chat only.** The batch path keeps fail-safe deny; only the
  interactive driver, which can actually prompt, relaxes to ask.

## Alternatives considered

- A `--cwd` flag to decouple the sandbox *root* from the persona workdir (scoped,
  keeps confinement). Rejected per the operator's choice: full-machine access
  with per-action `ask` is the desired model (mirrors how Claude Code itself
  works — broad FS reach, permission prompts).

## Bugs surfaced and fixed

- None. (A live model test showed gemma4 sometimes declining to call `read_file`
  in unrestricted mode — model caution, not a ctx bug; proven via the
  `dispatch_unconfined_reads_outside_workdir` integration test.)

## Status / deferred

- Shipped behind the flag; `bwoc-chat` uses it. A future `--cwd` scoped mode
  could be added if a middle ground is wanted.

## Related (links)

- `crates/bwoc-harness/src/tools/mod.rs` — `ToolContext::{new,unconfined}`, `resolve_path`
- `crates/bwoc-harness/src/main.rs` — `--unrestricted`, `chat_default_policy()`
- `projects/bwoc-chat` — passes `--unrestricted`
