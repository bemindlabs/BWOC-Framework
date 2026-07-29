# 2026-07-29 — bwoc-tui: multi-agent fleet mode (OpenCode-style)

Upgrades `bwoc-tui` from a single-agent chat client to an **OpenCode-style multi-agent** TUI: a left **fleet sidebar** (every workspace agent, `Tab` to switch, live status dot) beside the active agent's chat pane. Additive — the existing single-agent path (`bwoc chat <agent> --tui`) is untouched.

## Correction logged
The build began under a wrong premise ("bwoc-tui is an empty stub"). It is not — `lib.rs` was already a complete, tested single-agent TUI. So this is a **refactor that reuses the tested core** (`App::apply` event-mapping, the draw helpers, the `design` theming), not a rewrite. Two speculative duplicate files (`app.rs`, `ui.rs`) written under the wrong premise were discarded; only `session.rs` (a clean subprocess/proto abstraction) was kept.

## What changed
- **`session.rs`** (new) — `AgentInfo` + `fetch_fleet()` (`bwoc list --json`) and `Session`: spawns `bwoc-harness --chat` per agent, a reader thread parses each stdout line into a `ChatEvent` over an `mpsc`, `send()` writes `ChatInput` to stdin, `Drop` sends `Quit` + reaps.
- **`lib.rs`** — additive fleet layer: `Fleet` holds one `App` pane + one `Session` per agent (lazy), `drain()` folds **every** live session's events into its pane each tick (so a background agent keeps streaming into its sidebar dot), `Tab`/`BackTab` switch. `run_fleet()` + `draw_fleet()` reuse `draw_status`/`draw_body`/`draw_input` for the active pane and add a `draw_fleet_sidebar` (● streaming · ◍ unread · ○ idle).
- **CLI** — `bwoc chat <agent> --tui --fleet` (new `--fleet` flag, `requires = tui`). The named agent's backend + manifest model/endpoint seed the shared session config for the whole fleet.

## Decisions
- **Reuse the per-agent `App`, one per fleet member.** The tested single-agent event→state mapping becomes the per-pane unit for free; no duplicate rendering logic.
- **One backend/model for the session** (from the named agent), applied to all agents. Per-agent manifest resolution is deferred (P2) — matches OpenCode's single bottom model selector.
- **std threads + `mpsc`, no async runtime.** Consistent with the existing TUI; the workspace `tokio` lacks the `process` feature anyway. `drain()` uses `rx.try_iter()` (two-phase: collect under an immutable `sessions` borrow, then apply to `panes`) so the two maps never alias.
- **Layout B** (fleet sidebar + chat pane) chosen by the operator over a top tab-strip / split-panes.

## Verification
macOS: `cargo fmt` + `clippy --workspace -D warnings` clean; `bwoc-tui` + `bwoc-cli` tests pass (11 TUI tests, incl. the reused `apply`/status ones). No version drift. The underlying `--chat` chat_proto path + harness were **live-verified** earlier the same day (a real headless agent → `ask` → approval round-trip). The interactive fleet screen itself is not automatable in a non-TTY CI shell — the fleet layer is pure logic over that verified stream.

## Status / deferred (P2+)
- Per-agent backend/model from each manifest (today: one shared config).
- Inline tool-actions in the chat pane (today: reuses the conversation | activity split).
- `Ctrl-P` command palette, model/mode selector widget, `@agent` mention routing, persistent per-agent scrollback across a full app restart, `--team-chat` in fleet mode.

## Related
- `crates/bwoc-tui/src/{lib,session}.rs` · `crates/bwoc-cli/src/{chat,main}.rs` · `bwoc_core::chat_proto` · OpenCode (reference UX).
