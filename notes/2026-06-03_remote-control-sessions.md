# 2026-06-03 — `bwoc remote` — link & manage remote-control sessions

Added a CLI surface for linking agents to **remote-control sessions** (e.g.
Claude Code's Remote Control, drivable from claude.ai / mobile) and managing
those links. Requested as "implement link and manage claude remote control
sessions"; scoped — after clarification — as a **backend-neutral** bwoc-side
bookkeeping surface (option: CLI manager, not a daemon/RC-API integration).

## What changed

- **New module `crates/bwoc-cli/src/remote.rs`** + `Commands::Remote` wiring in `main.rs`:
  - `bwoc remote link <agent> <session-ref> [--backend --kind --url --note]` — write `.bwoc/remote/<agentId>.json`. `--backend` defaults from the agent manifest; `--kind` defaults to `claude-remote-control`.
  - `bwoc remote list` — table of all links (`--json`); flags orphaned links (agent no longer registered).
  - `bwoc remote status <agent>` — one link's detail (`--json`); exit 2 if the agent doesn't exist, 0 if it exists but is unlinked.
  - `bwoc remote unlink <agent>` — gated remove (TTY confirm unless `--yes`); tolerant of an already-removed/orphaned link.
- **`RemoteLink` schema** (`{ agentId, backend, kind, sessionRef, url?, linkedAt, note? }`) — serde-derived, stored per-agent under `.bwoc/remote/`, mirroring the `.bwoc/sessions/` marker convention.
- Docs: `WORKSPACE.en.md` (+ TH) CLI-surface row + "Remote-control session links" subsection; CHANGELOG `[Unreleased]`.

## Decisions

- **Backend-neutral, not Claude-special (Samānattatā).** The model is a generic "remote session" link; `kind` names the mechanism (`claude-remote-control` first). Any backend can declare its own kind without code changes — Claude is the first implementation, not a privileged path.
- **Bookkeeping only, not a proxy.** Per the chosen scope, `bwoc remote` records the association; it does not open, authenticate, or tunnel the actual RC session. That keeps it dependency-free and lets the real RC client live wherever it already does. A daemon-level "expose agent for RC" path and a real RC-API integration were the other two options considered and explicitly deferred.
- **`.bwoc/remote/<agentId>.json` per-agent files** (not one JSONL) — matches the existing `.bwoc/sessions/` marker pattern, makes `link`/`unlink` a single-file write/remove, and keeps `list` a simple directory read.
- **serde-derived struct**, unlike `sessions.rs`'s hand-rolled JSON — the crate already depends on `serde`/`serde_json` everywhere; hand-rolling buys nothing here.

## Bugs surfaced and fixed

- First test run flaked: the `tmp_ws()` helper built its temp dir from `pid + ISO-8601 timestamp` (second resolution), so two parallel tests starting in the same second shared a directory and one's `remove_dir_all` raced the other. Fixed with a per-call `AtomicU32` counter; re-ran the suite 3× clean.

## Status / deferred

- v1 is bookkeeping only. **Deferred until asked:** cross-referencing a link against live `bwoc sessions` state (show whether the linked agent is actually running), a daemon verb to expose an agent for RC, and any real Claude Code RC-API auth/token handling.
- Verified: `cargo test -p bwoc-cli remote` (4 pass, 3× no race), fmt + clippy (`-D warnings`) clean, and a full live lifecycle smoke test in a temp workspace (link with defaults + explicit backend/kind, list, status `--json`, unknown-agent exit 2, gated unlink, on-disk record).

## Related

- `crates/bwoc-cli/src/remote.rs`, `crates/bwoc-cli/src/sessions.rs` (marker-convention sibling)
- `docs/en/WORKSPACE.en.md` (+ `docs/th/WORKSPACE.th.md`) — CLI Surface
