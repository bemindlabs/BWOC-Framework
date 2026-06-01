# 2026-06-01 — Promote the chat TUI into a `bwoc-tui` crate

The `bwoc chat --tui` ratatui client moved out of `bwoc-cli` into its own crate, so the TUI can grow without bloating the CLI and the ratatui/crossterm surface stays isolated.

## What changed

- **New crate `crates/bwoc-tui`** (`bwoc-tui`), added to the workspace members + `[workspace.dependencies]`. Its `lib.rs` is the former `bwoc-cli/src/chat_tui.rs` (moved via `git mv`).
- Dependencies: `bwoc-core` (the `chat_proto` types + `exec::sibling_binary`), `ratatui`, `crossterm`, `serde_json`. **No `bwoc-cli`, no `bwoc-harness`** — verified with `cargo tree`.
- `bwoc-cli` now depends on `bwoc-tui`; the `bwoc chat --tui` branch calls `bwoc_tui::run(TuiArgs { agent_id, agent_path, backend_name })`. The `mod chat_tui;` and the file were removed from `bwoc-cli`.
- The 8 helper unit tests moved with the file and still pass.

## Decisions

- **Lean over Node/Ink.** Considered modelling on hermes-agent's `ui-tui/` (a separate React+Ink sub-project talking to a gateway). Rejected: it drags a whole Node/TS toolchain into a Rust framework (anti-Mattaññutā). The hermes split (renderer ↔ session-owner via a protocol) is already mirrored by `ratatui ↔ bwoc-harness --chat (chat_proto)`; promoting to a native crate keeps that without a new runtime. Not a new `projects/` sub-project either — `projects/` holds external reference clones, not BWOC's own code.
- **Crate cuts the last `bwoc-cli` tie itself.** The only CLI dependency was `Backend::harness_binary()`, which is just `bwoc_core::exec::sibling_binary("bwoc-harness")` since BWOC-15. So `bwoc-tui` resolves the harness directly via `bwoc-core` — no `Backend` enum, no `bwoc-cli` dep, no cycle.
- **`backend_name: String`, not `Backend`.** The pre-`Ready` status line wants the backend label; passing a plain `String` (the CLI's `display_name()`) avoids depending on `bwoc-cli`'s `Backend` enum. The harness's `Ready` event delivers the authoritative value moments later.

## Status / deferred

Pure refactor — no behavior change (the TUI works exactly as the merged #163 did). Future TUI growth (richer panes, scrollback, multi-session) now has a home. A persistent **session gateway** (the other thing hermes has — multi-session, any backend, one protocol) remains a separate, larger future step if universality beyond harness backends is wanted.

## Related (links)

- Origin of the TUI: #160 (harness `--chat` + `chat_proto`) + #163 (the ratatui client).
- Harness resolution reused: `bwoc_core::exec::sibling_binary` (BWOC-15).
